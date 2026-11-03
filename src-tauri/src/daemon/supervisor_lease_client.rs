//! Daemon-side supervisor lease client (plan todo 3, part B: relay I/O and wiring).
//!
//! Drives the pure [`RoleMachine`] of [`crate::daemon::supervisor_lease_role`]: it turns relay
//! session changes, one-second ticks and acquire/release commands into machine events, then
//! executes the returned [`Action`]s in order: start/stop the hub through a [`HubSwitch`], send
//! acquire/renew/release over the relay control channel (a [`LeaseTransport`]; the send time of
//! each request is passed back with its ack), and persist the snapshot to
//! `supervisor-lease.json` (0600, atomic write, on the blocking pool). It never touches PTYs and
//! never restarts the daemon.
//!
//! [`SupervisorLeaseState`] is the cheap, cloneable view side-effect gates call before acting;
//! its [`SupervisorLeaseState::is_holder_now`] re-checks the deadline on both clocks at the
//! moment of use through the machine, never a cached flag.
//!
//! Rolling handover (plan todo 9): at the commit decision the predecessor freezes its client
//! (no request is sent afterwards) and hands `{machine_id, fence, deadline}` to the successor
//! through the handover transaction record. The successor seeds its machine with that fence,
//! so its first request is a renew with the same machine id and fence, never an acquire, and
//! its hub generation starts only once that renew is granted.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::{mpsc, oneshot, watch};

use crate::daemon::handover_transaction::SupervisorLeaseTransfer;
use crate::daemon::supervisor_lease_role::{
    Action, Event, LastRole, LeaseAck, Now, PersistedLease, Role, RoleConfig, RoleMachine,
    StandDownReason,
};
use crate::remote::protocol::{
    SupervisorLeaseAck, SupervisorLeaseAcquire, SupervisorLeaseOutcome, SupervisorLeaseRelease,
    SupervisorLeaseQuery, SupervisorLeaseRenew, SupervisorLeaseRequest,
};
use crate::remote::relay_client::{LeaseCallError, RelayClient};

/// File under the remote data dir holding [`PersistedLease`].
pub const LEASE_FILE: &str = "supervisor-lease.json";
/// Marker left by `ferryx hub lease reset-capability`; a running client consumes it.
pub const RESET_CAPABILITY_MARKER: &str = "supervisor-lease.reset-capability";
/// Bound on one lease request's round trip over the control channel.
pub const LEASE_CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the first relay session may take before boot counts as relay-unreachable.
pub const BOOT_UNREACHABLE_AFTER: Duration = Duration::from_secs(10);
/// Tick cadence the machine expects ("about once a second").
const TICK: Duration = Duration::from_secs(1);
/// Requests one burst of actions may send before the client waits for the next event.
const MAX_SENDS_PER_EVENT: usize = 4;
/// Margin past the previous holder's lease deadline before a new holder opens Slack.
pub const TAKEOVER_MARGIN: Duration = Duration::from_secs(5);

/// The relay side as the client sees it: [`RelayClient`] in production, a fake in tests.
#[async_trait::async_trait]
pub trait LeaseTransport: Send + Sync + 'static {
    /// Lease capability of the live control session; `None` while disconnected.
    fn subscribe_capability(&self) -> watch::Receiver<Option<bool>>;
    async fn call(&self, request: SupervisorLeaseRequest) -> Result<SupervisorLeaseAck, LeaseCallError>;
}

#[async_trait::async_trait]
impl LeaseTransport for RelayClient {
    fn subscribe_capability(&self) -> watch::Receiver<Option<bool>> {
        self.subscribe_lease_capability()
    }

    async fn call(&self, request: SupervisorLeaseRequest) -> Result<SupervisorLeaseAck, LeaseCallError> {
        self.supervisor_lease_request(request, LEASE_CALL_TIMEOUT).await
    }
}

/// Starts and stops the hub when the machine asks; `stop_hub` is the stand-down event.
pub trait HubSwitch: Send + Sync + 'static {
    fn start_hub(&self);
    fn stop_hub(&self, reason: StandDownReason);
}

/// Both clocks as one reading.
pub type LeaseClock = Arc<dyn Fn() -> Now + Send + Sync>;

/// Production clock. The monotonic side goes through Tokio's clock so paused-time tests
/// drive it; outside tests it is `std::time::Instant::now()`.
pub fn system_clock() -> LeaseClock {
    Arc::new(|| Now {
        mono: tokio::time::Instant::now().into_std(),
        wall: SystemTime::now(),
    })
}

/// Path of `supervisor-lease.json`, `None` when the remote data dir cannot be resolved.
pub fn lease_file_path() -> Option<PathBuf> {
    crate::remote::auth::canonical_remote_dir().map(|dir| dir.join(LEASE_FILE))
}

/// What boot assumes when `supervisor-lease.json` exists but cannot be read or parsed: the
/// capability latch set and no held fence, so an enrolled machine starts as
/// Standby{unknown} and acquires through the relay, never Holder with fence 0.
fn unreadable_lease() -> PersistedLease {
    PersistedLease {
        last_role: LastRole::Standby,
        lease_capable: true,
        ..PersistedLease::default()
    }
}

/// Reads `supervisor-lease.json`; `None` only when it does not exist. A file that cannot be
/// read or parsed is logged and read as [`unreadable_lease`] (fail closed). Blocking.
pub fn read_persisted(path: &Path) -> Option<PersistedLease> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "supervisor lease state is unreadable; starting as standby");
            return Some(unreadable_lease());
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(lease) => Some(lease),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "supervisor lease state does not parse; starting as standby");
            Some(unreadable_lease())
        }
    }
}

/// Writes `supervisor-lease.json` (0600, atomic rename). Blocking.
pub fn write_persisted(path: &Path, lease: &PersistedLease) -> std::io::Result<()> {
    crate::account::store::write_private_json(path, lease)
}

/// Reads `supervisor-lease.json` on the blocking pool ([`read_persisted`]); a read that could
/// not run counts as unreadable.
pub async fn load_persisted(path: Option<PathBuf>) -> Option<PersistedLease> {
    let path = path?;
    match crate::ipc::run_blocking(move || Ok(read_persisted(&path))).await {
        Ok(lease) => lease,
        Err(error) => {
            tracing::warn!("supervisor lease state was not read: {}; starting as standby", error.message);
            Some(unreadable_lease())
        }
    }
}

/// `ferryx hub lease reset-capability`: clears the capability latch for a deliberate relay
/// downgrade, the same way the machine does on `CapabilityReset`, and leaves a marker so a
/// running daemon forgets its in-memory latch too. Blocking.
pub fn reset_capability_latch(path: &Path) -> std::io::Result<()> {
    let lease = PersistedLease {
        last_role: LastRole::Holder,
        last_fence: 0,
        holder_machine_id: None,
        lease_capable: false,
        ..read_persisted(path).unwrap_or_default()
    };
    write_persisted(path, &lease)?;
    let marker = path.with_file_name(RESET_CAPABILITY_MARKER);
    crate::account::store::write_private_json(&marker, &serde_json::json!({"reset": true}))
}

struct StateInner {
    account: Option<String>,
    machine_id: String,
    /// Validity of one grant counted from its request's send time (TTL - margin).
    valid_for: Duration,
    machine: parking_lot::Mutex<RoleMachine>,
    role_tx: watch::Sender<Role>,
    clock: LeaseClock,
    /// Fence and wall-clock stand-down deadline of the newest grant that made this machine
    /// Holder; what a handover hands to the successor.
    grant: parking_lot::Mutex<Option<(u64, SystemTime)>>,
    /// Set from the handover commit decision on: the client sends no lease request.
    handed_over: AtomicBool,
    /// From the last granted acquire that displaced another holder: when it was answered
    /// (local monotonic clock) and how long the previous holder's deadline could still run.
    takeover: parking_lot::Mutex<Option<(tokio::time::Instant, Duration)>>,
    /// The running client's command queue (weak, so the state never keeps the client alive);
    /// `None` for a state without a client.
    commands: Option<mpsc::WeakSender<LeaseCommand>>,
    /// Since when (monotonic, lease clock) this daemon has been on standby without a known
    /// holder; `None` while it knows one or runs the hub.
    no_holder_since: parking_lot::Mutex<Option<std::time::Instant>>,
    /// From the newest relay ack that carried them: the account machines the relay saw speak
    /// the lease protocol, and the account's enrolled machine count.
    lease_peers: parking_lot::Mutex<Option<LeasePeers>>,
}

/// The relay's view of which account machines speak the lease protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeasePeers {
    /// Relay machine ids, this machine included.
    pub capable: Vec<String>,
    pub account_machine_count: Option<u32>,
}

/// Standby that names no holder machine.
fn holder_unknown(role: &Role) -> bool {
    matches!(role, Role::Standby { holder_machine_id: None, holder_label: None, .. })
}

/// Cheap, cloneable view of the hub role shared with every side-effect gate.
#[derive(Clone)]
pub struct SupervisorLeaseState {
    inner: Arc<StateInner>,
}

impl SupervisorLeaseState {
    fn new(
        account: Option<String>,
        machine_id: String,
        valid_for: Duration,
        machine: RoleMachine,
        clock: LeaseClock,
        commands: Option<mpsc::WeakSender<LeaseCommand>>,
    ) -> Self {
        let (role_tx, _) = watch::channel(machine.role().clone());
        let no_holder_since = holder_unknown(machine.role()).then(|| clock().mono);
        Self {
            inner: Arc::new(StateInner {
                account,
                machine_id,
                valid_for,
                machine: parking_lot::Mutex::new(machine),
                role_tx,
                clock,
                grant: parking_lot::Mutex::new(None),
                handed_over: AtomicBool::new(false),
                takeover: parking_lot::Mutex::new(None),
                commands,
                no_holder_since: parking_lot::Mutex::new(no_holder_since),
                lease_peers: parking_lot::Mutex::new(None),
            }),
        }
    }

    /// Account this machine is enrolled in; `None` for account-less/selfhost.
    pub fn account(&self) -> Option<String> {
        self.inner.account.clone()
    }

    pub fn role(&self) -> Role {
        self.inner.machine.lock().role().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Role> {
        self.inner.role_tx.subscribe()
    }

    /// Fence to stamp on side effects: the holder's fence, 0 otherwise (0 = unleased mode).
    pub fn current_fence(&self) -> u64 {
        self.inner.machine.lock().fence().unwrap_or(0)
    }

    /// Strict holder check against both clocks read now (deadline and clock-jump rules of the
    /// machine); call before every side effect.
    pub fn is_holder_now(&self) -> bool {
        let now = (self.inner.clock)();
        self.inner.machine.lock().is_holder_now(now)
    }

    /// [`Self::is_holder_now`] or Provisional (offline single-machine hub).
    pub fn may_act_now(&self) -> bool {
        let now = (self.inner.clock)();
        self.inner.machine.lock().may_act_now(now)
    }

    /// The fence a release can be sent for right now (leased holder on an open capable
    /// session); `None` when no release could reach the relay.
    fn releasable_fence(&self) -> Option<u64> {
        let now = (self.inner.clock)();
        self.inner.machine.lock().releasable_fence(now)
    }

    /// Status hint, e.g. the `RELAY_LEASE_UNSUPPORTED` reset-capability hint.
    pub fn status_hint(&self) -> Option<&'static str> {
        self.inner.machine.lock().status_hint()
    }

    pub fn snapshot(&self) -> PersistedLease {
        self.inner.machine.lock().snapshot().clone()
    }

    /// Seconds until the held grant's local stand-down deadline (TTL - margin from its send
    /// time, wall clock); 0 without a leased grant for the current fence.
    pub fn lease_expires_in(&self) -> u64 {
        let fence = self.current_fence();
        let grant = *self.inner.grant.lock();
        match grant {
            Some((granted, deadline)) if fence != 0 && granted == fence => deadline
                .duration_since((self.inner.clock)().wall)
                .map_or(0, |left| left.as_secs()),
            _ => 0,
        }
    }

    /// How long this standby has known no holder; `None` while it knows one or runs the hub.
    pub fn no_holder_for(&self) -> Option<Duration> {
        let since = (*self.inner.no_holder_since.lock())?;
        Some((self.inner.clock)().mono.saturating_duration_since(since))
    }

    /// The newest relay answer naming the account machines that speak the lease protocol;
    /// `None` before one arrived or from a relay that predates the field.
    pub fn lease_peers(&self) -> Option<LeasePeers> {
        self.inner.lease_peers.lock().clone()
    }

    /// Keeps the lease-capable machines of `ack`; an ack without them changes nothing.
    fn note_lease_peers(&self, ack: &SupervisorLeaseAck) {
        if let Some(capable) = &ack.lease_capable_machines {
            *self.inner.lease_peers.lock() = Some(LeasePeers {
                capable: capable.clone(),
                account_machine_count: ack.account_machine_count,
            });
        }
    }

    /// The grant this machine holds right now (fence and wall-clock stand-down deadline), for
    /// a client rebound to a replaced relay to resume; `None` unless a leased holder now.
    pub fn live_grant(&self) -> Option<LiveGrant> {
        let fence = self.current_fence();
        if fence == 0 || !self.is_holder_now() {
            return None;
        }
        match *self.inner.grant.lock() {
            Some((granted, deadline)) if granted == fence => Some(LiveGrant { fence, deadline }),
            _ => None,
        }
    }

    /// True while a handover commit froze this client (no lease request is sent).
    pub fn handed_over(&self) -> bool {
        self.inner.handed_over.load(Ordering::SeqCst)
    }

    /// Handover commit decision: stops every further lease request and returns the held lease
    /// for the successor; `None` when this machine is not a leased holder right now (fence 0,
    /// standby, past the deadline), in which case the successor boots the normal way.
    pub fn freeze_for_handover(&self) -> Option<SupervisorLeaseTransfer> {
        // Frozen first, so no request is started after the lease was read.
        self.inner.handed_over.store(true, Ordering::SeqCst);
        let fence = self.current_fence();
        if fence == 0 || !self.is_holder_now() {
            return None;
        }
        let (granted, deadline) = (*self.inner.grant.lock())?;
        if granted != fence {
            return None;
        }
        let deadline_unix_ms = u64::try_from(deadline.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
        Some(SupervisorLeaseTransfer {
            machine_id: self.inner.machine_id.clone(),
            fence,
            deadline_unix_ms,
        })
    }

    /// The commit decision was not written: this daemon keeps the hub and renews again.
    pub fn thaw_after_failed_handover(&self) {
        self.inner.handed_over.store(false, Ordering::SeqCst);
    }

    /// How long this holder must still wait before it opens Slack after a takeover: the
    /// previous holder's remaining lease (`previous_expires_in_secs`) plus 5s, counted on the
    /// local monotonic clock from the acquire ack. Zero without a takeover or once it passed.
    pub fn takeover_wait(&self) -> Duration {
        let Some((acked_at, previous)) = *self.inner.takeover.lock() else {
            return Duration::ZERO;
        };
        (acked_at + previous + TAKEOVER_MARGIN).saturating_duration_since(tokio::time::Instant::now())
    }

    /// Starts the takeover wait from a granted acquire ack. A refusal (or any other outcome)
    /// grants nothing, so its `previous_expires_in_secs` never starts a wait.
    fn note_acquire_ack(&self, ack: &SupervisorLeaseAck) {
        if ack.outcome != SupervisorLeaseOutcome::Granted {
            return;
        }
        if let Some(previous) = ack.previous_expires_in_secs {
            *self.inner.takeover.lock() =
                Some((tokio::time::Instant::now(), Duration::from_secs(previous)));
        }
    }

    /// Feeds one event at the current time and publishes the role. The lock is never held
    /// across an await.
    fn handle(&self, event: Event) -> Vec<Action> {
        let now = (self.inner.clock)();
        let granted = match &event {
            Event::AcquireAck { sent_at, ack: LeaseAck::Granted { fence, .. } }
            | Event::RenewAck { sent_at, ack: LeaseAck::Granted { fence, .. } } => {
                Some((*fence, sent_at.wall + self.inner.valid_for))
            }
            _ => None,
        };
        let (actions, role) = {
            let mut machine = self.inner.machine.lock();
            let actions = machine.handle(event, now);
            (actions, machine.role().clone())
        };
        {
            let mut since = self.inner.no_holder_since.lock();
            if !holder_unknown(&role) {
                *since = None;
            } else if since.is_none() {
                *since = Some(now.mono);
            }
        }
        if let Some((fence, deadline)) = granted {
            if role == (Role::Holder { fence }) {
                let mut grant = self.inner.grant.lock();
                let newer = match *grant {
                    Some((held, current)) if held == fence => deadline > current,
                    _ => true,
                };
                if newer {
                    *grant = Some((fence, deadline));
                }
            }
        }
        self.inner.role_tx.send_if_modified(|current| {
            let changed = *current != role;
            if changed {
                *current = role;
            }
            changed
        });
        actions
    }
}

/// The fence supervisor inputs are stamped with: asked of the lease right before each send.
impl crate::agent_control::fence::SupervisorFenceSource for SupervisorLeaseState {
    fn account(&self) -> Option<String> {
        SupervisorLeaseState::account(self)
    }

    /// Both clocks read at the call, never a cached flag.
    fn is_holder_now(&self) -> bool {
        SupervisorLeaseState::is_holder_now(self)
    }

    /// A Provisional hub (offline single-machine boot) sends unfenced until its grant.
    fn may_act_now(&self) -> bool {
        SupervisorLeaseState::may_act_now(self)
    }

    /// 0 when this machine is not the holder.
    fn current_fence(&self) -> u64 {
        SupervisorLeaseState::current_fence(self)
    }

    /// A daemon refused this holder's fence: the hub stands down through the lease's own stop,
    /// and the client releases the lease (no more renewals, auto-acquire paused) so a later
    /// start cannot resurrect the generation on the stale fence.
    fn stand_down(&self) {
        crate::hub_runtime::stand_down_now();
        let commands = self.inner.commands.as_ref().and_then(mpsc::WeakSender::upgrade);
        if let Some(commands) = commands {
            if let Err(error) = commands.try_send(LeaseCommand::FenceStale) {
                tracing::warn!("stale-fence release not queued: {error}");
            }
        }
    }
}

/// The handover commit freezes this lease and hands it to the successor.
impl crate::daemon::handover::SupervisorLeaseHandover for SupervisorLeaseState {
    fn freeze_for_handover(&self) -> Option<SupervisorLeaseTransfer> {
        SupervisorLeaseState::freeze_for_handover(self)
    }

    fn thaw_after_failed_handover(&self) {
        SupervisorLeaseState::thaw_after_failed_handover(self)
    }
}

/// Requests other daemon tasks make of the lease client.
enum LeaseCommand {
    Acquire { force: bool, reply: oneshot::Sender<Role> },
    Release { reply: oneshot::Sender<Role> },
    /// Release reserving the fence for `target` (a move); replies whether the relay released.
    /// The role changes only on a `Released` ack.
    ReleaseTo { target: String, reply: oneshot::Sender<bool> },
    /// The relay's view of the lease (a move's first step); `None` without an answer.
    Query { reply: oneshot::Sender<Option<SupervisorLeaseAck>> },
    /// A daemon refused the held fence: release without a caller waiting.
    FenceStale,
}

/// Handle to a running lease client: the shared state plus acquire/release commands.
#[derive(Clone)]
pub struct SupervisorLeaseHandle {
    state: SupervisorLeaseState,
    commands: mpsc::Sender<LeaseCommand>,
}

impl SupervisorLeaseHandle {
    pub fn state(&self) -> &SupervisorLeaseState {
        &self.state
    }

    pub fn subscribe(&self) -> watch::Receiver<Role> {
        self.state.subscribe()
    }

    pub fn current_fence(&self) -> u64 {
        self.state.current_fence()
    }

    pub fn is_holder_now(&self) -> bool {
        self.state.is_holder_now()
    }

    /// Asks for the lease (`force` = takeover); returns the role once the request settled.
    /// `None` when the client task has ended.
    pub async fn acquire(&self, force: bool) -> Option<Role> {
        self.command(|reply| LeaseCommand::Acquire { force, reply }).await
    }

    /// Releases the lease and stands the hub down; auto-acquire pauses until [`Self::acquire`]
    /// or until the relay shows another machine holding the lease at a higher fence.
    pub async fn release(&self) -> Option<Role> {
        self.command(|reply| LeaseCommand::Release { reply }).await
    }

    /// [`Self::release`] reserving the released fence for `target` (the move target), with the
    /// last published checkpoint. True when the relay released the lease; only then does this
    /// machine stand down and pause auto-acquire. Without an open capable session, or when the
    /// relay does not answer `Released`, nothing is changed and the hub keeps running.
    pub async fn release_to(&self, target: &str) -> bool {
        let (reply, wait) = oneshot::channel();
        let command = LeaseCommand::ReleaseTo { target: target.to_owned(), reply };
        if self.commands.send(command).await.is_err() {
            return false;
        }
        wait.await.unwrap_or(false)
    }

    /// Asks the relay who holds the lease; `None` when it could not be asked.
    pub async fn query(&self) -> Option<SupervisorLeaseAck> {
        let (reply, wait) = oneshot::channel();
        self.commands.send(LeaseCommand::Query { reply }).await.ok()?;
        wait.await.ok().flatten()
    }

    async fn command(&self, build: impl FnOnce(oneshot::Sender<Role>) -> LeaseCommand) -> Option<Role> {
        let (reply, wait) = oneshot::channel();
        self.commands.send(build(reply)).await.ok()?;
        wait.await.ok()
    }
}

/// Inputs of [`spawn_supervisor_lease_client`] besides the transport and the hub switch.
pub struct LeaseClientConfig {
    pub role: RoleConfig,
    /// This machine's id (recorded as holder in the snapshot).
    pub machine_id: String,
    /// Account this machine is enrolled in; `None` for account-less/selfhost (Holder fence 0).
    pub account: Option<String>,
    /// `supervisor-lease.json`; `None` disables persistence.
    pub lease_file: Option<PathBuf>,
    pub clock: LeaseClock,
    /// The lease a handover predecessor handed this daemon at commit; `None` otherwise.
    pub handed_over: Option<SupervisorLeaseTransfer>,
    /// Checkpoints this machine holds and publishes (acquire/renew/release carry their ids).
    pub checkpoints: Arc<crate::supervisor_checkpoint::CheckpointState>,
    /// The live grant of the client this one replaces (a rebind to a replaced relay); the new
    /// client starts as Holder on it instead of standing the running hub down.
    pub resumed: Option<LiveGrant>,
}

/// A held grant: its fence and its wall-clock stand-down deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveGrant {
    pub fence: u64,
    pub deadline: SystemTime,
}

impl LeaseClientConfig {
    /// Production settings: standard timings, the canonical lease file, the system clocks.
    pub fn system(machine_id: impl Into<String>, account: Option<String>) -> Self {
        Self {
            role: RoleConfig::default(),
            machine_id: machine_id.into(),
            account,
            lease_file: lease_file_path(),
            clock: system_clock(),
            handed_over: None,
            checkpoints: crate::supervisor_checkpoint::shared(),
            resumed: None,
        }
    }
}

/// Seeds the persisted lease with a lease handed over at handover commit: Holder with the
/// predecessor's fence, so the machine's first request is a renew of that fence. Ignored when
/// it names another machine, carries no fence, or its deadline already passed (normal boot).
fn adopt_handed_lease(
    persisted: Option<PersistedLease>,
    handed: Option<&SupervisorLeaseTransfer>,
    machine_id: &str,
    now: SystemTime,
) -> Option<PersistedLease> {
    let usable = handed.filter(|lease| {
        lease.machine_id == machine_id
            && lease.fence > 0
            && UNIX_EPOCH
                .checked_add(Duration::from_millis(lease.deadline_unix_ms))
                .is_some_and(|deadline| deadline > now)
    });
    let Some(lease) = usable else { return persisted };
    Some(PersistedLease {
        last_role: LastRole::Holder,
        last_fence: lease.fence,
        holder_machine_id: Some(machine_id.to_owned()),
        lease_capable: true,
        ..persisted.unwrap_or_default()
    })
}

/// Loads the persisted lease, builds the role machine and spawns the client task, which runs
/// until every handle is dropped. The first relay session not showing up within
/// [`BOOT_UNREACHABLE_AFTER`] counts as relay-unreachable at boot (provisional rule).
pub async fn spawn_supervisor_lease_client<T, H>(
    config: LeaseClientConfig,
    transport: Arc<T>,
    hub: Arc<H>,
) -> (SupervisorLeaseHandle, tokio::task::JoinHandle<()>)
where
    T: LeaseTransport + ?Sized,
    H: HubSwitch + ?Sized,
{
    let persisted = load_persisted(config.lease_file.clone()).await;
    let persisted = adopt_handed_lease(
        persisted,
        config.handed_over.as_ref(),
        &config.machine_id,
        (config.clock)().wall,
    );
    let valid_for = config.role.ttl.saturating_sub(config.role.margin);
    let (mut machine, mut initial) = RoleMachine::new(
        config.role,
        config.machine_id.clone(),
        config.account.is_some(),
        persisted,
        (config.clock)(),
    );
    // A rebind keeps the grant the replaced client held, so the holder's generation runs on.
    let resumed = config.resumed.and_then(|grant| {
        let actions = machine.resume_grant(grant.fence, grant.deadline, (config.clock)())?;
        initial.extend(actions);
        Some((grant.fence, grant.deadline))
    });
    let (commands, command_rx) = mpsc::channel(8);
    let state = SupervisorLeaseState::new(
        config.account,
        config.machine_id,
        valid_for,
        machine,
        config.clock.clone(),
        Some(commands.downgrade()),
    );
    *state.inner.grant.lock() = resumed;
    // Subscribed before the task starts, so no capability change can slip past it.
    let capability = transport.subscribe_capability();
    let driver = Driver {
        state: state.clone(),
        transport,
        hub,
        lease_file: config.lease_file,
        clock: config.clock,
        checkpoints: config.checkpoints,
    };
    let task = tokio::spawn(driver.run(initial, command_rx, capability));
    (SupervisorLeaseHandle { state, commands }, task)
}

struct Driver<T: ?Sized, H: ?Sized> {
    state: SupervisorLeaseState,
    transport: Arc<T>,
    hub: Arc<H>,
    lease_file: Option<PathBuf>,
    clock: LeaseClock,
    checkpoints: Arc<crate::supervisor_checkpoint::CheckpointState>,
}

impl<T: LeaseTransport + ?Sized, H: HubSwitch + ?Sized> Driver<T, H> {
    async fn run(
        self,
        initial: Vec<Action>,
        mut commands: mpsc::Receiver<LeaseCommand>,
        mut capability: watch::Receiver<Option<bool>>,
    ) {
        self.execute(initial, false).await;
        let mut session_seen = false;
        let first = *capability.borrow_and_update();
        self.on_capability(first, &mut session_seen).await;
        let boot_deadline = tokio::time::sleep(BOOT_UNREACHABLE_AFTER);
        tokio::pin!(boot_deadline);
        let mut boot_pending = !session_seen;
        let mut capability_open = true;
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                // Session changes first, so a command or tick never runs against a stale session.
                biased;
                changed = capability.changed(), if capability_open => {
                    let advertised = if changed.is_ok() {
                        *capability.borrow_and_update()
                    } else {
                        // The transport is gone: disconnected for good.
                        capability_open = false;
                        None
                    };
                    self.on_capability(advertised, &mut session_seen).await;
                    boot_pending &= !session_seen;
                }
                _ = &mut boot_deadline, if boot_pending => {
                    boot_pending = false;
                    self.feed(Event::RelayUnreachableAtBoot).await;
                }
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    let (event, reply) = match command {
                        LeaseCommand::Acquire { force, reply } => (Event::AcquireRequested { force }, Some(reply)),
                        LeaseCommand::Release { reply } => (Event::ReleaseRequested, Some(reply)),
                        LeaseCommand::FenceStale => (Event::ReleaseRequested, None),
                        LeaseCommand::ReleaseTo { target, reply } => {
                            let _ = reply.send(self.release_to(target).await);
                            continue;
                        }
                        LeaseCommand::Query { reply } => {
                            let query = SupervisorLeaseRequest::SupervisorLeaseQuery(SupervisorLeaseQuery {
                                request_id: request_id(),
                            });
                            let ack = self.transport.call(query).await.ok();
                            if let Some(ack) = &ack {
                                self.state.note_lease_peers(ack);
                            }
                            let _ = reply.send(ack);
                            continue;
                        }
                    };
                    self.feed_with(event, true).await;
                    if let Some(reply) = reply {
                        let _ = reply.send(self.state.role());
                    }
                }
                _ = ticker.tick() => {
                    if self.consume_reset_marker().await {
                        self.feed(Event::CapabilityReset).await;
                    }
                    self.feed(Event::Tick).await;
                }
            }
        }
    }

    /// `Some(capable)` = a control session is up, `None` = it closed.
    async fn on_capability(&self, advertised: Option<bool>, session_seen: &mut bool) {
        match advertised {
            Some(lease_capable) => {
                *session_seen = true;
                self.feed(Event::SessionOpened { lease_capable }).await;
            }
            None if *session_seen => self.feed(Event::SessionClosed).await,
            None => {}
        }
    }

    async fn feed(&self, event: Event) {
        self.feed_with(event, false).await;
    }

    /// `explicit`: a person's or the hub's command, not the machine's own cadence.
    async fn feed_with(&self, event: Event, explicit: bool) {
        let actions = self.state.handle(event);
        self.execute(actions, explicit).await;
    }

    /// Executes actions in order; acks of sent requests go back into the machine with the
    /// request's send time and their actions run in the same burst (bounded). A standby whose
    /// owner stopped the hub never takes part in auto-promotion: only `explicit` acquires go out.
    /// A standby that knows another holder asks the relay first and auto-acquires only when
    /// its checkpoint state allows it ([`Self::auto_acquire_allowed`]).
    /// A standby paused after a release asks the relay who holds the lease
    /// ([`Action::SendQuery`]); the answer goes back as [`Event::QueryAck`].
    async fn execute(&self, actions: Vec<Action>, explicit: bool) {
        let mut queue: VecDeque<Action> = actions.into();
        let mut sends = 0;
        while let Some(action) = queue.pop_front() {
            // Auto-promotion: a standby asks at its cadence with its newest checkpoint and the
            // relay (single writer) grants a free lease only against the recorded checkpoint;
            // a machine whose owner stopped the hub does not take part.
            if matches!(action, Action::SendAcquire { .. })
                && !explicit
                && matches!(self.state.role(), Role::Standby { .. })
                && self.checkpoints.hub_stopped()
            {
                continue;
            }
            if matches!(action, Action::SendAcquire { force: false })
                && !explicit
                && matches!(self.state.role(), Role::Standby { holder_machine_id: Some(_), .. })
                && !self.auto_acquire_allowed().await
            {
                continue;
            }
            // `renew` selects the ack event: RenewAck for a renew, AcquireAck for an acquire;
            // `reported` is the checkpoint id a renew carries.
            let (request, renew, reported) = match action {
                Action::StartHub => {
                    self.hub.start_hub();
                    continue;
                }
                Action::StopHub { reason } => {
                    self.hub.stop_hub(reason);
                    continue;
                }
                Action::Persist(snapshot) => {
                    self.persist(snapshot).await;
                    continue;
                }
                Action::SendQuery => {
                    if self.state.handed_over() {
                        continue;
                    }
                    let query = SupervisorLeaseRequest::SupervisorLeaseQuery(SupervisorLeaseQuery {
                        request_id: request_id(),
                    });
                    match self.transport.call(query).await {
                        Ok(ack) => {
                            self.state.note_lease_peers(&ack);
                            let ack = LeaseAck::from_wire(&ack);
                            queue.extend(self.state.handle(Event::QueryAck { ack }));
                        }
                        Err(error) => tracing::debug!("supervisor lease query failed: {error}"),
                    }
                    continue;
                }
                Action::SendAcquire { force } => (
                    SupervisorLeaseRequest::SupervisorLeaseAcquire(SupervisorLeaseAcquire {
                        request_id: request_id(),
                        // The relay grants a takeover only against the checkpoint it recorded.
                        checkpoint_id: self.checkpoints.local().map(|local| local.id),
                        force,
                    }),
                    false,
                    None,
                ),
                Action::SendRenew { fence } => {
                    let reported = self.checkpoints.unreported();
                    let renew = SupervisorLeaseRequest::SupervisorLeaseRenew(SupervisorLeaseRenew {
                        request_id: request_id(),
                        fence,
                        checkpoint_id: reported.clone(),
                    });
                    (renew, true, reported)
                }
                Action::SendRelease { fence } => {
                    if self.state.handed_over() {
                        tracing::debug!("supervisor lease handed over; release not sent");
                        continue;
                    }
                    let release = SupervisorLeaseRequest::SupervisorLeaseRelease(SupervisorLeaseRelease {
                        request_id: request_id(),
                        fence,
                        checkpoint_id: self.checkpoints.published(),
                        reserved_for: None,
                    });
                    // A release ack has no role effect; failures leave the relay lease to expire.
                    match self.transport.call(release).await {
                        Ok(ack) if ack.outcome == SupervisorLeaseOutcome::Released => {}
                        Ok(ack) => tracing::warn!(
                            outcome = ?ack.outcome,
                            reason = ?ack.reason_code,
                            "supervisor lease release not acknowledged"
                        ),
                        Err(error) => tracing::warn!("supervisor lease release not acknowledged: {error}"),
                    }
                    continue;
                }
            };
            // After the handover commit decision the successor renews this lease; the
            // predecessor sends nothing more.
            if self.state.handed_over() {
                tracing::debug!("supervisor lease handed over; request not sent");
                continue;
            }
            if sends >= MAX_SENDS_PER_EVENT {
                tracing::warn!("supervisor lease request deferred to the next tick");
                continue;
            }
            sends += 1;
            let sent_at = (self.clock)();
            let was_holder = matches!(self.state.role(), Role::Holder { fence } if fence > 0);
            match self.transport.call(request).await {
                Ok(ack) => {
                    self.state.note_lease_peers(&ack);
                    if !renew {
                        self.state.note_acquire_ack(&ack);
                    }
                    if renew {
                        if let Some(id) = reported.filter(|id| ack.checkpoint_id.as_deref() == Some(id.as_str())) {
                            self.checkpoints.mark_reported(&id);
                        }
                    } else if let Some(refusal) = crate::supervisor_checkpoint::refusal_of(&ack) {
                        self.checkpoints.note_refusal(Some(refusal));
                    } else if matches!(ack.outcome, SupervisorLeaseOutcome::Granted | SupervisorLeaseOutcome::Renewed) {
                        self.checkpoints.note_granted(ack.checkpoint_id.as_deref(), was_holder);
                    }
                    let ack = LeaseAck::from_wire(&ack);
                    let event = if renew {
                        Event::RenewAck { sent_at, ack }
                    } else {
                        Event::AcquireAck { sent_at, ack }
                    };
                    queue.extend(self.state.handle(event));
                }
                // No ack: the machine retries at its cadence and the deadline rules apply.
                Err(error) => tracing::debug!("supervisor lease request failed: {error}"),
            }
        }
    }

    /// A move's release: sent only while this machine holds a releasable lease on an open
    /// capable session, reserving the fence for `target` with the last published checkpoint.
    /// Only a `Released` ack stands the hub down and pauses auto-acquire
    /// ([`Event::ReleaseAcknowledged`]); any other answer, or none, leaves the role, the
    /// running hub and auto-acquire as they were. Returns whether the relay released.
    async fn release_to(&self, target: String) -> bool {
        if self.state.handed_over() {
            tracing::debug!("supervisor lease handed over; move release not sent");
            return false;
        }
        let Some(fence) = self.state.releasable_fence() else {
            tracing::warn!("supervisor lease move release not sent: no held lease on an open relay session");
            return false;
        };
        let release = SupervisorLeaseRequest::SupervisorLeaseRelease(SupervisorLeaseRelease {
            request_id: request_id(),
            fence,
            checkpoint_id: self.checkpoints.published(),
            reserved_for: Some(target),
        });
        match self.transport.call(release).await {
            Ok(ack) if ack.outcome == SupervisorLeaseOutcome::Released => {
                self.feed_with(Event::ReleaseAcknowledged, true).await;
                true
            }
            Ok(ack) => {
                tracing::warn!(
                    outcome = ?ack.outcome,
                    reason = ?ack.reason_code,
                    "supervisor lease move release refused; this machine keeps the supervisor"
                );
                false
            }
            Err(error) => {
                tracing::warn!("supervisor lease move release not acknowledged; this machine keeps the supervisor: {error}");
                false
            }
        }
    }

    /// Auto-promotion's condition, asked at the machine's acquire cadence (every renew period
    /// while standby): the relay's view of the lease goes into the checkpoint state, which
    /// allows the acquire only when the lease is free and records no checkpoint or this
    /// machine's newest one, or names this machine as its holder (the relay's own-row rule),
    /// and the owner has not stopped the hub here. No answer: no acquire.
    async fn auto_acquire_allowed(&self) -> bool {
        if self.state.handed_over() {
            return false;
        }
        let query = SupervisorLeaseRequest::SupervisorLeaseQuery(SupervisorLeaseQuery { request_id: request_id() });
        match self.transport.call(query).await {
            Ok(ack) => {
                self.state.note_lease_peers(&ack);
                self.checkpoints.note_query(&ack, &self.state.inner.machine_id);
                self.checkpoints.may_auto_acquire()
            }
            Err(error) => {
                tracing::debug!("supervisor lease query failed: {error}");
                false
            }
        }
    }

    async fn persist(&self, snapshot: PersistedLease) {
        let Some(path) = self.lease_file.clone() else { return };
        match crate::ipc::run_blocking(move || Ok(write_persisted(&path, &snapshot))).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("supervisor lease state not saved: {error}"),
            Err(error) => tracing::warn!("supervisor lease state not saved: {}", error.message),
        }
    }

    /// True once per `ferryx hub lease reset-capability`: the marker is removed when seen.
    async fn consume_reset_marker(&self) -> bool {
        let Some(path) = self.lease_file.clone() else { return false };
        let marker = path.with_file_name(RESET_CAPABILITY_MARKER);
        crate::ipc::run_blocking(move || Ok(std::fs::remove_file(marker).is_ok()))
            .await
            .unwrap_or(false)
    }
}

fn request_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::supervisor_lease as lease;
    use crate::daemon::supervisor_lease_role::RELAY_LEASE_UNSUPPORTED;
    use crate::remote::protocol::SupervisorLeaseOutcome;
    use parking_lot::Mutex;
    use tokio::time::Instant;

    /// Full validity of a grant with the standard config: TTL 120s - 15s margin.
    const VALID_FOR: Duration = Duration::from_secs(105);

    /// Fake relay: records each request with its send instant and answers from a script.
    struct FakeRelay {
        capability: watch::Sender<Option<bool>>,
        sent: Mutex<Vec<(Instant, SupervisorLeaseRequest)>>,
        acquire: Mutex<Result<SupervisorLeaseOutcome, LeaseCallError>>,
        renew: Mutex<Result<SupervisorLeaseOutcome, LeaseCallError>>,
        /// What a release answers (`Released` by default).
        release: Mutex<Result<SupervisorLeaseOutcome, LeaseCallError>>,
        /// Fence an acquire answers with (renewals answer 7, the fence they renew).
        acquire_fence: Mutex<u64>,
        /// What a lease query answers (outcome, recorded checkpoint); `None` answers it like
        /// an acquire.
        query: Mutex<Option<(SupervisorLeaseOutcome, Option<String>)>>,
        /// The previous holder's remaining lease an acquire ack reports (a takeover).
        previous_expires: Mutex<Option<u64>>,
        delay: Duration,
    }

    impl FakeRelay {
        fn new(advertised: Option<bool>) -> Arc<Self> {
            Self::delayed(advertised, Duration::ZERO)
        }

        fn delayed(advertised: Option<bool>, delay: Duration) -> Arc<Self> {
            let (capability, _) = watch::channel(advertised);
            Arc::new(Self {
                capability,
                sent: Mutex::new(Vec::new()),
                acquire: Mutex::new(Ok(SupervisorLeaseOutcome::Granted)),
                renew: Mutex::new(Ok(SupervisorLeaseOutcome::Renewed)),
                release: Mutex::new(Ok(SupervisorLeaseOutcome::Released)),
                acquire_fence: Mutex::new(7),
                query: Mutex::new(None),
                previous_expires: Mutex::new(None),
                delay,
            })
        }

        fn first_send(&self) -> Instant {
            self.sent.lock().first().expect("a request was sent").0
        }

        fn renews(&self) -> usize {
            self.sent
                .lock()
                .iter()
                .filter(|(_, r)| matches!(r, SupervisorLeaseRequest::SupervisorLeaseRenew(_)))
                .count()
        }

        fn acquires(&self) -> usize {
            self.sent
                .lock()
                .iter()
                .filter(|(_, r)| matches!(r, SupervisorLeaseRequest::SupervisorLeaseAcquire(_)))
                .count()
        }

        fn queries(&self) -> usize {
            self.sent
                .lock()
                .iter()
                .filter(|(_, r)| matches!(r, SupervisorLeaseRequest::SupervisorLeaseQuery(_)))
                .count()
        }
    }

    #[async_trait::async_trait]
    impl LeaseTransport for FakeRelay {
        fn subscribe_capability(&self) -> watch::Receiver<Option<bool>> {
            self.capability.subscribe()
        }

        async fn call(&self, request: SupervisorLeaseRequest) -> Result<SupervisorLeaseAck, LeaseCallError> {
            let request_id = request.request_id().to_owned();
            let scripted_query = match request {
                SupervisorLeaseRequest::SupervisorLeaseQuery(_) => self.query.lock().clone(),
                _ => None,
            };
            if let Some((outcome, checkpoint_id)) = scripted_query {
                self.sent.lock().push((Instant::now(), request));
                return Ok(SupervisorLeaseAck {
                    request_id,
                    outcome,
                    fence: Some(7),
                    holder_machine_id: Some("mach-b".into()),
                    holder_label: Some("Laptop B".into()),
                    expires_in_secs: None,
                    previous_expires_in_secs: None,
                    checkpoint_id,
                    account_machine_count: Some(2),
                    reason_code: None,
                    lease_capable_machines: None,
                });
            }
            let fence = if matches!(request, SupervisorLeaseRequest::SupervisorLeaseAcquire(_)) {
                *self.acquire_fence.lock()
            } else {
                7
            };
            let previous_expires_in_secs = if matches!(request, SupervisorLeaseRequest::SupervisorLeaseAcquire(_)) {
                *self.previous_expires.lock()
            } else {
                None
            };
            let script = match request {
                SupervisorLeaseRequest::SupervisorLeaseRenew(_) => self.renew.lock().clone(),
                SupervisorLeaseRequest::SupervisorLeaseRelease(_) => self.release.lock().clone(),
                _ => self.acquire.lock().clone(),
            };
            self.sent.lock().push((Instant::now(), request));
            tokio::time::sleep(self.delay).await;
            let outcome = script?;
            let refused = outcome == SupervisorLeaseOutcome::Refused;
            let reason_code = match outcome {
                SupervisorLeaseOutcome::Refused => Some("LEASE_HELD".into()),
                SupervisorLeaseOutcome::Unavailable => Some("ACCOUNT_STORE_READ_ONLY".into()),
                _ => None,
            };
            Ok(SupervisorLeaseAck {
                request_id,
                outcome,
                fence: Some(fence),
                holder_machine_id: refused.then(|| "mach-b".into()),
                holder_label: refused.then(|| "Laptop B".into()),
                expires_in_secs: Some(120),
                previous_expires_in_secs,
                checkpoint_id: None,
                account_machine_count: Some(2),
                reason_code,
                lease_capable_machines: None,
            })
        }
    }

    #[derive(Default)]
    struct FakeHub {
        events: Mutex<Vec<Option<StandDownReason>>>,
    }

    impl HubSwitch for FakeHub {
        fn start_hub(&self) {
            self.events.lock().push(None);
        }
        fn stop_hub(&self, reason: StandDownReason) {
            self.events.lock().push(Some(reason));
        }
    }

    /// Monotonic side follows Tokio's (paused) clock; the wall side adds a settable jump.
    fn test_clock() -> (LeaseClock, Arc<Mutex<Duration>>) {
        let start = Instant::now();
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let jump = Arc::new(Mutex::new(Duration::ZERO));
        let reader = Arc::clone(&jump);
        let clock: LeaseClock = Arc::new(move || {
            let now = Instant::now();
            Now {
                mono: now.into_std(),
                wall: base + (now - start) + *reader.lock(),
            }
        });
        (clock, jump)
    }

    fn config(clock: LeaseClock, account: Option<&str>, lease_file: Option<PathBuf>) -> LeaseClientConfig {
        LeaseClientConfig {
            role: RoleConfig::default(),
            machine_id: "mach-a".into(),
            account: account.map(str::to_owned),
            lease_file,
            clock,
            handed_over: None,
            checkpoints: Arc::default(),
            resumed: None,
        }
    }

    /// The lease a predecessor hands over: `mach-a`, `fence`, deadline a full validity after
    /// the test clock's start.
    fn handed(fence: u64) -> SupervisorLeaseTransfer {
        SupervisorLeaseTransfer {
            machine_id: "mach-a".into(),
            fence,
            deadline_unix_ms: (1_700_000_000 + VALID_FOR.as_secs()) * 1000,
        }
    }

    async fn wait_role(handle: &SupervisorLeaseHandle, wanted: impl Fn(&Role) -> bool) -> Role {
        let mut roles = handle.subscribe();
        let role = tokio::time::timeout(Duration::from_secs(600), roles.wait_for(|role| wanted(role)))
            .await
            .expect("role reached in bounded (paused) time")
            .expect("lease client alive")
            .clone();
        role
    }

    #[tokio::test(start_paused = true)]
    async fn a_capable_relay_grants_holder_and_renewals_keep_it() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        assert_eq!(wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await, Role::Holder { fence: 7 });
        assert_eq!(handle.current_fence(), 7);
        assert!(handle.is_holder_now());
        assert_eq!(handle.state().account().as_deref(), Some("acct"));
        assert!(handle.state().snapshot().lease_capable, "capability latched");
        let first = relay.first_send();
        // Well past the first grant's deadline: only renewals can keep the role.
        tokio::time::sleep_until(first + VALID_FOR * 3).await;
        assert!(relay.renews() >= 3);
        assert!(handle.is_holder_now());
        assert_eq!(handle.state().role(), Role::Holder { fence: 7 });
        assert!(!hub.events.lock().iter().any(Option::is_some), "never stood down");
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn missed_renewals_stand_down_exactly_at_the_deadline() {
        let relay = FakeRelay::new(Some(true));
        *relay.renew.lock() = Err(LeaseCallError::Timeout);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        let deadline = relay.first_send() + VALID_FOR;
        tokio::time::sleep_until(deadline - Duration::from_secs(1)).await;
        assert!(handle.is_holder_now(), "one second before the deadline");
        tokio::time::sleep_until(deadline).await;
        assert!(!handle.is_holder_now(), "re-checked at use, exactly at the deadline");
        wait_role(&handle, |r| *r == Role::standby_unknown()).await;
        assert!(hub.events.lock().contains(&Some(StandDownReason::DeadlineReached)));
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_handover_successor_renews_the_handed_fence_without_an_acquire() {
        let relay = FakeRelay::new(Some(true));
        // An acquire would be granted a new fence; only the handed-over one may be used.
        *relay.acquire_fence.lock() = 8;
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let mut successor = config(clock, Some("acct"), None);
        successor.handed_over = Some(handed(7));
        let (handle, task) =
            spawn_supervisor_lease_client(successor, Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, Role::runs_hub).await;
        assert_eq!(handle.state().role(), Role::Holder { fence: 7 }, "same fence as the predecessor");
        assert!(handle.is_holder_now());
        assert!(matches!(
            relay.sent.lock().first(),
            Some((_, SupervisorLeaseRequest::SupervisorLeaseRenew(SupervisorLeaseRenew { fence: 7, .. })))
        ));
        assert_eq!(relay.acquires(), 0, "no acquire, so no new fence");
        assert_eq!(*hub.events.lock(), vec![None], "hub started once, after the renew was granted");
        task.abort();
    }

    /// Rolling handover on paused time: the predecessor renews until the commit decision, then
    /// sends nothing; the successor renews the same fence after booting. Every request renews
    /// the relay row, so the row never expires when no gap between requests reaches the TTL.
    #[tokio::test(start_paused = true)]
    async fn the_lease_crosses_a_handover_without_a_gap_and_the_predecessor_goes_quiet() {
        let ttl = Duration::from_secs(crate::account::supervisor_lease::TTL_SECS);
        let (clock, _) = test_clock();
        let old_relay = FakeRelay::new(Some(true));
        let (old, old_task) = spawn_supervisor_lease_client(
            config(clock.clone(), Some("acct"), None),
            Arc::clone(&old_relay),
            Arc::new(FakeHub::default()),
        )
        .await;
        wait_role(&old, |r| *r == Role::Holder { fence: 7 }).await;
        // The predecessor keeps renewing up to the commit.
        tokio::time::sleep(Duration::from_secs(50)).await;
        assert!(old_relay.renews() >= 2);

        let transfer = old.state().freeze_for_handover().expect("a held lease is handed over");
        assert_eq!((transfer.machine_id.as_str(), transfer.fence), ("mach-a", 7));
        let sent_at_commit = old_relay.sent.lock().len();

        // The successor finishes its handover start before its hub (and lease client) starts.
        tokio::time::sleep(Duration::from_secs(30)).await;
        let new_relay = FakeRelay::new(Some(true));
        *new_relay.acquire_fence.lock() = 8;
        let mut successor = config(clock, Some("acct"), None);
        successor.handed_over = Some(transfer);
        let (new, new_task) =
            spawn_supervisor_lease_client(successor, Arc::clone(&new_relay), Arc::new(FakeHub::default())).await;
        wait_role(&new, |r| *r == Role::Holder { fence: 7 }).await;
        tokio::time::sleep(ttl * 3).await;

        assert_eq!(old_relay.sent.lock().len(), sent_at_commit, "predecessor sent nothing after commit");
        assert!(old.state().handed_over());
        assert!(new.is_holder_now());
        assert_eq!(new_relay.acquires(), 0);
        let mut sends: Vec<Instant> = old_relay.sent.lock().iter().map(|(at, _)| *at).collect();
        sends.extend(new_relay.sent.lock().iter().map(|(at, _)| *at));
        sends.sort();
        assert!(sends.windows(2).all(|pair| pair[1] - pair[0] < ttl), "relay row never expires");
        old_task.abort();
        new_task.abort();
    }

    /// A client rebound to a replaced relay resumes the replaced client's live grant: it is
    /// Holder on the same fence from the start (no stand-down while the new relay connects)
    /// and renews that fence, never acquiring a new one, once the new relay's session is up.
    #[tokio::test(start_paused = true)]
    async fn a_rebound_client_keeps_the_live_grant_and_renews_its_fence() {
        let (clock, _) = test_clock();
        let old_relay = FakeRelay::new(Some(true));
        let (old, old_task) = spawn_supervisor_lease_client(
            config(clock.clone(), Some("acct"), None),
            Arc::clone(&old_relay),
            Arc::new(FakeHub::default()),
        )
        .await;
        wait_role(&old, |r| *r == Role::Holder { fence: 7 }).await;
        let grant = old.state().live_grant().expect("a held grant");
        assert_eq!(grant.fence, 7);
        old_task.abort();

        // The replacement relay is not connected yet.
        let new_relay = FakeRelay::new(None);
        *new_relay.acquire_fence.lock() = 8;
        let hub = Arc::new(FakeHub::default());
        let mut rebound = config(clock, Some("acct"), None);
        rebound.resumed = Some(grant);
        let (new, new_task) =
            spawn_supervisor_lease_client(rebound, Arc::clone(&new_relay), Arc::clone(&hub)).await;
        assert_eq!(new.state().role(), Role::Holder { fence: 7 }, "holder from the start");
        assert!(new.is_holder_now());
        assert_eq!(new.state().live_grant(), Some(grant), "the grant's deadline carried over");
        // Past the boot-unreachable window: still holder on the carried grant.
        tokio::time::sleep(BOOT_UNREACHABLE_AFTER + Duration::from_secs(1)).await;
        assert!(new.is_holder_now());
        new_relay.capability.send_replace(Some(true));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(new_relay.renews() >= 1, "the carried fence is renewed");
        assert_eq!(new_relay.acquires(), 0, "no acquire, so no new fence");
        assert_eq!(new.state().role(), Role::Holder { fence: 7 });
        assert!(!hub.events.lock().iter().any(Option::is_some), "never stood down");
        new_task.abort();
    }

    #[test]
    fn a_handed_lease_is_adopted_only_for_this_machine_before_its_deadline() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let adopted = adopt_handed_lease(None, Some(&handed(7)), "mach-a", now).expect("adopted");
        assert_eq!((adopted.last_role, adopted.last_fence), (LastRole::Holder, 7));
        assert_eq!(adopted.holder_machine_id.as_deref(), Some("mach-a"));
        assert!(adopted.lease_capable);

        let persisted = PersistedLease { account_machine_count: Some(2), ..Default::default() };
        let foreign = adopt_handed_lease(Some(persisted.clone()), Some(&handed(7)), "mach-b", now);
        assert_eq!(foreign, Some(persisted.clone()), "another machine's lease is not adopted");
        let late = now + VALID_FOR;
        assert_eq!(adopt_handed_lease(Some(persisted.clone()), Some(&handed(7)), "mach-a", late), Some(persisted));
        assert_eq!(adopt_handed_lease(None, Some(&handed(0)), "mach-a", now), None);
    }

    #[tokio::test(start_paused = true)]
    async fn freezing_hands_nothing_over_unless_the_lease_is_held() {
        let (clock, _) = test_clock();
        let (handle, task) = spawn_supervisor_lease_client(
            config(clock, None, None),
            FakeRelay::new(Some(true)),
            Arc::new(FakeHub::default()),
        )
        .await;
        assert_eq!(handle.state().role(), Role::Holder { fence: 0 });
        assert_eq!(handle.state().freeze_for_handover(), None, "fence 0 is nothing to hand over");
        handle.state().thaw_after_failed_handover();
        assert!(!handle.state().handed_over());
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_counts_from_the_send_time_of_a_delayed_ack() {
        let relay = FakeRelay::delayed(Some(true), Duration::from_secs(10));
        *relay.renew.lock() = Err(LeaseCallError::Timeout);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), hub).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        let sent = relay.first_send();
        assert_eq!(Instant::now() - sent, Duration::from_secs(10), "ack arrived 10s after send");
        tokio::time::sleep_until(sent + VALID_FOR - Duration::from_secs(1)).await;
        assert!(handle.is_holder_now());
        tokio::time::sleep_until(sent + VALID_FOR).await;
        assert!(!handle.is_holder_now(), "deadline is send + TTL - margin, not ack + TTL - margin");
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_wall_clock_jump_ends_holder_before_the_next_tick() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, jump) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        // Sleep/wake: one hour of wall time with no monotonic advance.
        *jump.lock() = Duration::from_secs(3600);
        assert!(!handle.is_holder_now());
        assert!(!handle.state().may_act_now());
        task.abort();
    }

    /// The acceptance case for holder-only side effects: the wall clock jumps past the lease
    /// deadline and no tick runs. A queued chat reply is not posted (it stays queued) and a
    /// goal settle is held, because each side effect re-checks the lease right before it.
    /// The client is stopped before the jump: on paused time the runtime auto-advances while
    /// sqlx waits on its worker thread, and a tick would renew the lease (a renew the relay
    /// grants makes this machine the holder again). Only the re-check at use is left to deny.
    /// Time is resumed once the client is gone: on paused time that same auto-advance runs
    /// out sqlx's pool acquire timeout while SQLite opens (slow on Windows), and with no
    /// client left, real time can no longer renew anything.
    /// The reply goes to a fake transport, so a post that skipped the check would be seen.
    #[tokio::test(start_paused = true)]
    async fn past_the_deadline_without_a_tick_a_reply_stays_queued_and_no_goal_settles() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, jump) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled(), "no tick can run from here on");
        tokio::time::resume();
        let state = handle.state().clone();
        let gate: ferryx_hub::holder::HolderGate = Arc::new(move || state.may_act_now());
        assert!(gate(), "the holder may act before the jump");
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let session = ferryx_hub::chat::session::SessionKey::new("slack", None::<String>, "C1", None::<String>, "U1");
        let transport = Arc::new(FakeChat::default());
        *jump.lock() = VALID_FOR * 2;
        let work = ferryx_hub::holder::scope(gate, async {
            let id = ferryx_hub::chat::reply::chat_reply(&pool, &session, "done", None).await.unwrap();
            let delivered = ferryx_hub::chat::reply::deliver_stored(&pool, id, &session.storage_key(), "done")
                .await
                .unwrap();
            assert!(!delivered, "a standby posts nothing");
            let settle = ferryx_hub::supervisor::receipts::record_pty_settle(&pool, "local", "pane-1", "i1", 1).await;
            (ferryx_hub::chat::reply::pending(&pool).await.unwrap(), settle)
        });
        let (queued, settle) = ferryx_hub::chat::reply::with_transport(transport.clone(), work).await;
        assert_eq!(queued.len(), 1, "the reply stays queued for the holder");
        assert_eq!(*transport.sent.lock(), 0, "nothing reached the chat transport");
        assert_eq!(settle.unwrap_err().code(), ferryx_hub::holder::SUPERVISOR_STANDBY);
    }

    /// Chat transport that counts what it was asked to post.
    #[derive(Default)]
    struct FakeChat {
        sent: Mutex<usize>,
    }

    #[async_trait::async_trait]
    impl ferryx_hub::chat::router::Outbound for FakeChat {
        async fn send(&self, _session: &ferryx_hub::chat::session::SessionKey, _text: &str) -> ferryx_hub::error::Result<()> {
            *self.sent.lock() += 1;
            Ok(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_takeover_ack_delays_slack_until_the_old_deadline_plus_five_seconds() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), hub).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        assert_eq!(handle.state().takeover_wait(), Duration::ZERO, "no takeover, no wait");
        let ack = SupervisorLeaseAck {
            request_id: "r".into(),
            outcome: SupervisorLeaseOutcome::Granted,
            fence: Some(8),
            holder_machine_id: None,
            holder_label: None,
            expires_in_secs: Some(120),
            previous_expires_in_secs: Some(90),
            checkpoint_id: None,
            account_machine_count: Some(2),
            reason_code: None,
            lease_capable_machines: None,
        };
        handle.state().note_acquire_ack(&ack);
        assert_eq!(handle.state().takeover_wait(), Duration::from_secs(95));
        tokio::time::advance(Duration::from_secs(94)).await;
        assert_eq!(handle.state().takeover_wait(), Duration::from_secs(1), "still inside the old lease");
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(handle.state().takeover_wait(), Duration::ZERO);
        task.abort();
    }

    /// The driver records the takeover a relay acquire ack reports: the grant that displaced
    /// another holder (`previous_expires_in_secs`) starts the Slack wait, through the real
    /// acquire path rather than a direct `note_acquire_ack`.
    #[tokio::test(start_paused = true)]
    async fn a_granted_takeover_ack_from_the_relay_starts_the_takeover_wait() {
        let relay = FakeRelay::new(Some(true));
        *relay.previous_expires.lock() = Some(90);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), hub).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        let wait = handle.state().takeover_wait();
        assert!(wait > Duration::from_secs(90), "the old deadline plus the margin: {wait:?}");
        assert!(wait <= Duration::from_secs(95), "{wait:?}");
        task.abort();
    }

    /// Only a granted acquire starts the takeover wait: a refusal carrying the holder's
    /// remaining lease does not.
    #[tokio::test(start_paused = true)]
    async fn a_refused_acquire_ack_does_not_start_a_takeover_wait() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), hub).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        let refused = SupervisorLeaseAck {
            request_id: "r".into(),
            outcome: SupervisorLeaseOutcome::Refused,
            fence: Some(8),
            holder_machine_id: Some("mach-b".into()),
            holder_label: Some("Laptop B".into()),
            expires_in_secs: None,
            previous_expires_in_secs: Some(90),
            checkpoint_id: None,
            account_machine_count: Some(2),
            reason_code: None,
            lease_capable_machines: None,
        };
        handle.state().note_acquire_ack(&refused);
        assert_eq!(handle.state().takeover_wait(), Duration::ZERO, "a refusal starts no wait");
        task.abort();
    }

    /// The lease-capable machines of the newest ack that carries them are kept with the
    /// account machine count; an ack from a relay without the field leaves them as they were.
    #[tokio::test(start_paused = true)]
    async fn the_state_keeps_the_lease_capable_machines_of_the_newest_ack_carrying_them() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), hub).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        assert_eq!(handle.state().lease_peers(), None, "the fake relay predates the field");
        let ack = |capable: Option<Vec<String>>| SupervisorLeaseAck {
            request_id: "q".into(),
            outcome: SupervisorLeaseOutcome::Granted,
            fence: Some(7),
            holder_machine_id: None,
            holder_label: None,
            expires_in_secs: None,
            previous_expires_in_secs: None,
            checkpoint_id: None,
            account_machine_count: Some(3),
            reason_code: None,
            lease_capable_machines: capable,
        };
        handle.state().note_lease_peers(&ack(Some(vec!["mach-a".into()])));
        let expected = LeasePeers { capable: vec!["mach-a".into()], account_machine_count: Some(3) };
        assert_eq!(handle.state().lease_peers(), Some(expected.clone()));
        handle.state().note_lease_peers(&ack(None));
        assert_eq!(handle.state().lease_peers(), Some(expected));
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_acquire_is_standby_with_the_holder_label() {
        let relay = FakeRelay::new(Some(true));
        *relay.acquire.lock() = Ok(SupervisorLeaseOutcome::Refused);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        let role = wait_role(&handle, |r| matches!(r, Role::Standby { holder_label: Some(_), .. })).await;
        assert_eq!(
            role,
            Role::Standby {
                holder_machine_id: Some("mach-b".into()),
                holder_label: Some("Laptop B".into()),
                fence: Some(7),
            }
        );
        assert!(!handle.is_holder_now());
        assert_eq!(handle.current_fence(), 0);
        assert!(hub.events.lock().contains(&Some(StandDownReason::Refused)));
        // Refusal is retried at the renew cadence, not in a tight loop.
        let sent_before = relay.sent.lock().len();
        tokio::time::sleep(Duration::from_secs(10)).await;
        assert!(relay.sent.lock().len() <= sent_before + 1);
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_relay_without_the_capability_keeps_holder_fence_zero() {
        let relay = FakeRelay::new(Some(false));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(handle.state().role(), Role::Holder { fence: 0 });
        assert!(handle.is_holder_now());
        assert!(relay.sent.lock().is_empty(), "no lease traffic to an incapable relay");
        assert_eq!(*hub.events.lock(), vec![None], "hub started once at boot");
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn an_account_less_daemon_is_holder_fence_zero_even_on_a_capable_relay() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, None, None), Arc::clone(&relay), hub).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(handle.state().role(), Role::Holder { fence: 0 });
        assert!(relay.sent.lock().is_empty());
        task.abort();
    }

    #[tokio::test]
    async fn capability_seen_then_missing_is_unleased_with_the_reset_hint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LEASE_FILE);
        write_persisted(&path, &PersistedLease { lease_capable: true, ..Default::default() }).unwrap();
        let relay = FakeRelay::new(Some(false));
        let hub = Arc::new(FakeHub::default());
        let (handle, task) = spawn_supervisor_lease_client(
            config(system_clock(), Some("acct"), Some(path.clone())),
            Arc::clone(&relay),
            hub,
        )
        .await;
        let role = wait_role(&handle, |r| matches!(r, Role::Unleased { .. })).await;
        assert_eq!(role, Role::Unleased { reason: RELAY_LEASE_UNSUPPORTED.into() });
        assert!(!handle.is_holder_now());
        assert!(handle.state().status_hint().is_some_and(|hint| hint.contains("reset-capability")));
        // The CLI clears the latch; the running client picks the marker up on its next tick.
        reset_capability_latch(&path).unwrap();
        let role = wait_role(&handle, |r| *r == Role::Holder { fence: 0 }).await;
        assert_eq!(role, Role::Holder { fence: 0 });
        assert!(!dir.path().join(RESET_CAPABILITY_MARKER).exists(), "marker consumed");
        assert!(!read_persisted(&path).unwrap().lease_capable);
        task.abort();
    }

    #[tokio::test]
    async fn a_grant_is_persisted_with_the_account_machine_count() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LEASE_FILE);
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (handle, task) = spawn_supervisor_lease_client(
            config(system_clock(), Some("acct"), Some(path.clone())),
            Arc::clone(&relay),
            hub,
        )
        .await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        // A command round trip orders after the persist that preceded the published role.
        handle.acquire(false).await;
        let persisted = read_persisted(&path).expect("lease file written");
        assert_eq!(persisted.last_role, LastRole::Holder);
        assert_eq!(persisted.last_fence, 7);
        assert_eq!(persisted.account_machine_count, Some(2));
        assert_eq!(persisted.holder_machine_id.as_deref(), Some("mach-a"));
        assert!(persisted.lease_capable);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn release_sends_the_fence_and_stands_down() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        assert_eq!(handle.release().await, Some(Role::standby_unknown()));
        assert!(relay.sent.lock().iter().any(|(_, r)| matches!(
            r,
            SupervisorLeaseRequest::SupervisorLeaseRelease(SupervisorLeaseRelease { fence: 7, .. })
        )));
        assert!(hub.events.lock().contains(&Some(StandDownReason::Released)));
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn an_unreachable_relay_at_boot_is_provisional_for_a_single_machine_holder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LEASE_FILE);
        let single = PersistedLease {
            last_role: LastRole::Holder,
            last_fence: 4,
            account_machine_count: Some(1),
            holder_machine_id: Some("mach-a".into()),
            lease_capable: true,
        };
        write_persisted(&path, &single).unwrap();
        let relay = FakeRelay::new(None);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), Some(path)), relay, Arc::clone(&hub)).await;
        assert_eq!(handle.state().role(), Role::standby_unknown());
        wait_role(&handle, |r| *r == Role::Provisional).await;
        assert!(handle.state().may_act_now());
        assert!(!handle.is_holder_now(), "provisional is not a strict holder");
        assert_eq!(*hub.events.lock(), vec![None]);
        task.abort();
    }

    #[test]
    fn reset_capability_clears_the_latch_and_leaves_a_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LEASE_FILE);
        let latched = PersistedLease { lease_capable: true, account_machine_count: Some(3), ..Default::default() };
        write_persisted(&path, &latched).unwrap();
        reset_capability_latch(&path).unwrap();
        let lease = read_persisted(&path).unwrap();
        assert!(!lease.lease_capable);
        assert_eq!(lease.account_machine_count, Some(3));
        assert!(dir.path().join(RESET_CAPABILITY_MARKER).exists());
    }

    /// Lease state on a clock the test sets: both clocks at `base + secs`.
    fn state_at(account: Option<&str>, persisted: Option<PersistedLease>) -> (SupervisorLeaseState, Arc<Mutex<u64>>) {
        let base = Now {
            mono: std::time::Instant::now(),
            wall: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        };
        let secs = Arc::new(Mutex::new(0_u64));
        let reader = Arc::clone(&secs);
        let clock: LeaseClock = Arc::new(move || {
            let offset = Duration::from_secs(*reader.lock());
            Now { mono: base.mono + offset, wall: base.wall + offset }
        });
        let (machine, _) = RoleMachine::new(RoleConfig::default(), "mach-a", account.is_some(), persisted, clock());
        let state =
            SupervisorLeaseState::new(account.map(str::to_owned), "mach-a".into(), VALID_FOR, machine, clock, None);
        (state, secs)
    }

    #[test]
    fn a_fence_source_past_its_deadline_refuses_the_send() {
        use crate::agent_control::fence::{stamp, FenceRefusal, SupervisorFence, SupervisorFenceSource};
        let latched = PersistedLease { last_role: LastRole::Standby, lease_capable: true, ..Default::default() };
        let (state, secs) = state_at(Some("acct"), Some(latched));
        state.handle(Event::SessionOpened { lease_capable: true });
        let sent_at = (state.inner.clock)();
        state.handle(Event::AcquireAck {
            sent_at,
            ack: LeaseAck::Granted { fence: 5, account_machine_count: Some(2) },
        });
        let source: &dyn SupervisorFenceSource = &state;
        assert!(source.is_holder_now());
        assert_eq!(stamp(Some(source)), Ok(Some(SupervisorFence { account: "acct".into(), fence: 5 })));
        // Past send + TTL - margin with no tick fed since: the source answers from the clocks.
        *secs.lock() = VALID_FOR.as_secs();
        assert!(!source.is_holder_now());
        assert_eq!(stamp(Some(source)), Err(FenceRefusal::NotHolder));
        assert_eq!(state.role(), Role::Holder { fence: 5 }, "no tick ran; the role is not what refused");
    }

    /// The fence push the hub runtime wires (`DaemonFencePush`) as seen from one lease state:
    /// stamps against it, records pushes and forwards a stand-down to it.
    struct StatePush {
        state: SupervisorLeaseState,
        pushed: Mutex<Vec<u64>>,
    }

    #[async_trait::async_trait]
    impl ferryx_hub::supervisor::fence_gate::FencePush for StatePush {
        async fn hosts(&self) -> Vec<String> {
            vec!["paired-1".into()]
        }

        async fn push_fence(&self, _host_id: Option<&str>) -> Result<(), String> {
            use crate::agent_control::fence::{stamp, SupervisorFenceSource};
            match stamp(Some(&self.state as &dyn SupervisorFenceSource)) {
                Ok(Some(fence)) => {
                    self.pushed.lock().push(fence.fence);
                    Ok(())
                }
                Ok(None) => Ok(()),
                Err(refusal) => Err(format!("{} {}", refusal.code(), refusal.message())),
            }
        }

        fn stand_down(&self) {
            crate::agent_control::fence::SupervisorFenceSource::stand_down(&self.state);
        }

        fn unfenced_now(&self) -> bool {
            use crate::agent_control::fence::{stamp, SupervisorFenceSource};
            matches!(stamp(Some(&self.state as &dyn SupervisorFenceSource)), Ok(None))
        }
    }

    fn releases(relay: &FakeRelay) -> usize {
        relay
            .sent
            .lock()
            .iter()
            .filter(|(_, r)| matches!(r, SupervisorLeaseRequest::SupervisorLeaseRelease(_)))
            .count()
    }

    #[tokio::test(start_paused = true)]
    async fn a_provisional_boot_pushes_unfenced_and_never_releases() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LEASE_FILE);
        let single = PersistedLease {
            last_role: LastRole::Holder,
            last_fence: 4,
            account_machine_count: Some(1),
            holder_machine_id: Some("mach-a".into()),
            lease_capable: true,
        };
        write_persisted(&path, &single).unwrap();
        let relay = FakeRelay::new(None);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), Some(path)), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Provisional).await;
        let push = Arc::new(StatePush { state: handle.state().clone(), pushed: Mutex::new(Vec::new()) });
        let gate = ferryx_hub::supervisor::fence_gate::FenceGate::new(push.clone());

        gate.push_on_start().await;
        assert_eq!(
            crate::agent_control::fence::stamp(Some(
                handle.state() as &dyn crate::agent_control::fence::SupervisorFenceSource
            )),
            Ok(None),
            "a provisional hub sends unfenced"
        );
        assert!(gate.ensure(Some("paired-1")).await.is_ok(), "provisional dispatch is not blocked");
        assert!(gate.unreachable_hosts().is_empty());
        assert!(push.pushed.lock().is_empty(), "no fence to push before a grant");
        tokio::time::sleep(VALID_FOR * 2).await;
        assert!(!gate.stood_down());
        assert_eq!(handle.state().role(), Role::Provisional, "never released");
        assert_eq!(releases(&relay), 0);
        assert_eq!(*hub.events.lock(), vec![None], "hub started once, never stopped");
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_clock_jump_then_dispatch_waits_without_releasing_the_lease() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, jump) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        let push = Arc::new(StatePush { state: handle.state().clone(), pushed: Mutex::new(Vec::new()) });
        let gate = ferryx_hub::supervisor::fence_gate::FenceGate::new(push.clone());
        gate.push_on_start().await;
        assert_eq!(*push.pushed.lock(), vec![7, 7], "own daemon and the paired host");

        // Sleep/wake right before a dispatch, no tick yet: the send is refused as not holder.
        *jump.lock() = Duration::from_secs(3600);
        let refused = crate::agent_control::fence::stamp(Some(
            handle.state() as &dyn crate::agent_control::fence::SupervisorFenceSource,
        ))
        .unwrap_err();
        assert_eq!(refused.code(), ferryx_hub::agent_control::SUPERVISOR_NOT_HOLDER);
        let fresh = ferryx_hub::supervisor::fence_gate::FenceGate::new(push.clone());
        let why = fresh.ensure(None).await.unwrap_err();
        assert!(why.starts_with(ferryx_hub::agent_control::SUPERVISOR_NOT_HOLDER), "{why}");
        assert!(!fresh.stood_down() && !gate.stood_down(), "nothing stands down");

        // The jump is renewed through and the lease is held again; nothing was released.
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        tokio::time::sleep(VALID_FOR * 2).await;
        assert!(handle.is_holder_now());
        assert_eq!(releases(&relay), 0, "the lease was never released");
        assert!(fresh.ensure(None).await.is_ok(), "the waiting dispatch goes out once held");
        assert!(!hub.events.lock().contains(&Some(StandDownReason::Released)));
        task.abort();
    }

    #[tokio::test]
    async fn a_reset_capability_latch_keeps_dispatching_unfenced_past_a_stored_maximum() {
        use crate::agent_control::fence::{stamp, FenceRatchet, SupervisorFence, SupervisorFenceSource};
        let reset = PersistedLease {
            last_role: LastRole::Holder,
            last_fence: 0,
            account_machine_count: Some(2),
            holder_machine_id: None,
            lease_capable: false,
        };
        let (state, _) = state_at(Some("acct"), Some(reset));
        assert_eq!(state.role(), Role::Holder { fence: 0 });
        let ratchet = FenceRatchet::load(None);
        ratchet.admit(&SupervisorFence { account: "acct".into(), fence: 9 }).await.unwrap();
        // Unfenced: the daemon's stored maximum of 9 cannot refuse it.
        assert_eq!(stamp(Some(&state as &dyn SupervisorFenceSource)), Ok(None));
        assert_eq!(ratchet.max_for("acct").await, 9);
    }

    #[test]
    fn an_unreadable_lease_file_is_standby_never_holder_fence_zero() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LEASE_FILE);
        assert_eq!(read_persisted(&path), None, "a missing file is a fresh install");
        std::fs::write(&path, b"{ truncated").unwrap();
        let read = read_persisted(&path).expect("an unreadable file is not a missing one");
        assert!(read.lease_capable);
        assert_eq!((read.last_role, read.last_fence), (LastRole::Standby, 0));
        let (state, _) = state_at(Some("acct"), Some(read.clone()));
        assert_eq!(state.role(), Role::standby_unknown(), "enrolled: standby, hub not started");
        assert_eq!(state.current_fence(), 0);
        // Without an enrollment there is no lease to arbitrate: today's unleased holder.
        let (unenrolled, _) = state_at(None, Some(read));
        assert_eq!(unenrolled.role(), Role::Holder { fence: 0 });
    }

    #[tokio::test]
    async fn a_corrupt_lease_file_boots_an_enrolled_daemon_as_standby() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LEASE_FILE);
        std::fs::write(&path, b"not json").unwrap();
        let loaded = load_persisted(Some(path)).await.expect("read as unreadable, not missing");
        assert!(loaded.lease_capable);
        assert_eq!(loaded.last_role, LastRole::Standby);
    }

    /// Status reads: how long a standby has known no holder, then the grant's time left.
    #[test]
    fn status_reads_how_long_no_holder_is_known_and_the_grant_time_left() {
        let latched = PersistedLease { last_role: LastRole::Standby, lease_capable: true, ..Default::default() };
        let (state, secs) = state_at(Some("acct"), Some(latched));
        assert_eq!(state.no_holder_for(), Some(Duration::ZERO));
        assert_eq!(state.lease_expires_in(), 0);
        *secs.lock() = 30;
        assert_eq!(state.no_holder_for(), Some(Duration::from_secs(30)));

        state.handle(Event::SessionOpened { lease_capable: true });
        let sent_at = (state.inner.clock)();
        state.handle(Event::AcquireAck {
            sent_at,
            ack: LeaseAck::Granted { fence: 5, account_machine_count: Some(2) },
        });

        assert_eq!(state.role(), Role::Holder { fence: 5 });
        assert_eq!(state.no_holder_for(), None);
        assert_eq!(state.lease_expires_in(), VALID_FOR.as_secs());
        *secs.lock() = 70;
        assert_eq!(state.lease_expires_in(), VALID_FOR.as_secs() - 40);
    }

    #[test]
    fn a_fence_gate_stand_down_reaches_the_hub_stop_signal() {
        use crate::agent_control::fence::{dispatcher_fence_gate, install_source, SupervisorFenceSource};
        // Account-less, so every other stamp stays unfenced while this source is installed.
        let (state, _) = state_at(None, None);
        let (wanted, mut decisions) = watch::channel(true);
        crate::hub_runtime::install_lease_stop(Some(Arc::new(wanted)));
        let source: Arc<dyn SupervisorFenceSource> = Arc::new(state);
        install_source(Some(source));
        let gate = dispatcher_fence_gate(crate::daemon::client::DaemonClient::new(), None);

        gate.stand_down();

        assert!(decisions.has_changed().expect("stop signal alive"));
        assert!(!*decisions.borrow_and_update(), "the hub was told to stop");
        assert!(gate.stood_down());
        install_source(None);
        crate::hub_runtime::install_lease_stop(None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_stale_fence_stand_down_releases_and_stops_renewing() {
        a_stale_fence_stand_down_case().await;
    }

    fn checkpoint(id: char, pulled: bool) -> crate::supervisor_checkpoint::StoredCheckpoint {
        crate::supervisor_checkpoint::StoredCheckpoint { id: id.to_string().repeat(64), at: 1, pulled }
    }

    #[tokio::test(start_paused = true)]
    async fn renews_report_a_published_checkpoint_and_a_move_release_reserves_the_target() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let cfg = config(clock, Some("acct"), None);
        let checkpoints = Arc::clone(&cfg.checkpoints);
        let (handle, task) = spawn_supervisor_lease_client(cfg, Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        checkpoints.publish(checkpoint('a', false));
        let published = "a".repeat(64);

        tokio::time::sleep(Duration::from_secs(lease::RENEW_SECS + 1)).await;

        assert!(relay.sent.lock().iter().any(|(_, r)| matches!(
            r,
            SupervisorLeaseRequest::SupervisorLeaseRenew(SupervisorLeaseRenew { checkpoint_id: Some(id), .. }) if *id == published
        )));
        assert!(handle.release_to("mach-b").await, "the fake relay released");
        assert!(relay.sent.lock().iter().any(|(_, r)| matches!(
            r,
            SupervisorLeaseRequest::SupervisorLeaseRelease(SupervisorLeaseRelease {
                fence: 7,
                checkpoint_id: Some(id),
                reserved_for: Some(target),
                ..
            }) if *id == published && target == "mach-b"
        )));
        assert_eq!(handle.state().role(), Role::standby_unknown());
        assert!(hub.events.lock().contains(&Some(StandDownReason::Released)));
        // Auto-acquire paused after the move: no acquire goes out at the cadence.
        let acquires = relay.acquires();
        tokio::time::sleep(Duration::from_secs(lease::RENEW_SECS * 3)).await;
        assert_eq!(relay.acquires(), acquires);
        task.abort();
    }

    /// After a move release the paused standby asks who holds the lease: the target holding
    /// it at a higher fence is named and makes this machine an auto-promotion candidate again.
    #[tokio::test(start_paused = true)]
    async fn after_a_move_the_new_holder_is_named_and_auto_acquire_resumes() {
        let relay = FakeRelay::new(Some(true));
        *relay.acquire_fence.lock() = 5;
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 5 }).await;
        assert!(handle.release_to("mach-b").await);
        // The target took the reserved lease at fence 7.
        *relay.query.lock() = Some((SupervisorLeaseOutcome::Refused, None));
        let role = wait_role(&handle, |r| matches!(r, Role::Standby { holder_machine_id: Some(_), .. })).await;
        assert_eq!(
            role,
            Role::Standby {
                holder_machine_id: Some("mach-b".into()),
                holder_label: Some("Laptop B".into()),
                fence: Some(7),
            }
        );
        assert_eq!(handle.state().snapshot().holder_machine_id.as_deref(), Some("mach-b"));
        assert_eq!(relay.acquires(), 1, "the holder is live: no acquire");
        // The holder's lease lapses: this machine auto-acquires again.
        *relay.query.lock() = Some((SupervisorLeaseOutcome::Released, None));
        wait_role(&handle, |r| *r == Role::Holder { fence: 5 }).await;
        assert_eq!(relay.acquires(), 2);
        task.abort();
    }

    /// A free lease, or one still at the released fence (the reservation pending), keeps a
    /// released standby paused and its holder unknown.
    #[tokio::test(start_paused = true)]
    async fn after_a_move_a_free_or_reserved_lease_keeps_auto_acquire_paused() {
        for outcome in [SupervisorLeaseOutcome::Released, SupervisorLeaseOutcome::Refused] {
            let relay = FakeRelay::new(Some(true));
            let hub = Arc::new(FakeHub::default());
            let (clock, _) = test_clock();
            let (handle, task) =
                spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
            wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
            assert!(handle.release_to("mach-b").await);
            // The query answers fence 7, the released one.
            *relay.query.lock() = Some((outcome, None));
            tokio::time::sleep(Duration::from_secs(lease::RENEW_SECS * 3)).await;
            assert!(relay.queries() >= 2, "{outcome:?}");
            assert_eq!(relay.acquires(), 1, "{outcome:?}");
            assert_eq!(handle.state().role(), Role::standby_unknown(), "{outcome:?}");
            task.abort();
        }
    }

    fn move_releases(relay: &FakeRelay) -> usize {
        relay
            .sent
            .lock()
            .iter()
            .filter(|(_, r)| matches!(r, SupervisorLeaseRequest::SupervisorLeaseRelease(_)))
            .count()
    }

    /// A move release the relay does not acknowledge as `Released` (refused with
    /// `ACCOUNT_STORE_READ_ONLY`, refused outright, or no answer) changes nothing here: the
    /// holder keeps its role and running hub, renews on, and auto-acquire stays on.
    #[tokio::test(start_paused = true)]
    async fn a_move_release_the_relay_did_not_acknowledge_keeps_the_holder() {
        for failure in [
            Ok(SupervisorLeaseOutcome::Unavailable),
            Ok(SupervisorLeaseOutcome::Refused),
            Err(LeaseCallError::Timeout),
        ] {
            let relay = FakeRelay::new(Some(true));
            let hub = Arc::new(FakeHub::default());
            let (clock, _) = test_clock();
            let (handle, task) =
                spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
            wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
            *relay.release.lock() = failure.clone();

            assert!(!handle.release_to("mach-b").await, "{failure:?}");

            assert_eq!(move_releases(&relay), 1, "the release was sent: {failure:?}");
            assert_eq!(handle.state().role(), Role::Holder { fence: 7 }, "{failure:?}");
            assert!(handle.is_holder_now());
            assert_eq!(*hub.events.lock(), vec![None], "the hub was never stopped: {failure:?}");
            assert_eq!(handle.state().snapshot().last_role, LastRole::Holder);
            // Still renewing well past the first grant's deadline.
            let renews = relay.renews();
            tokio::time::sleep(VALID_FOR * 2).await;
            assert!(relay.renews() > renews, "renewals continue: {failure:?}");
            assert_eq!(handle.state().role(), Role::Holder { fence: 7 });
            // A plain release still stands down as before.
            assert_eq!(handle.release().await, Some(Role::standby_unknown()));
            task.abort();
        }
    }

    /// Losing the lease after a refused move: auto-acquire was never paused, so the standby
    /// asks for the lease again at its cadence.
    #[tokio::test(start_paused = true)]
    async fn a_refused_move_release_leaves_auto_acquire_on() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        *relay.release.lock() = Ok(SupervisorLeaseOutcome::Unavailable);
        assert!(!handle.release_to("mach-b").await);

        // The renew is refused: standby with no fence; the next attempt acquires again.
        *relay.renew.lock() = Ok(SupervisorLeaseOutcome::Refused);
        wait_role(&handle, |r| matches!(r, Role::Standby { .. })).await;
        let acquires = relay.acquires();
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        assert!(relay.acquires() > acquires, "auto-acquire stayed on after the refused move");
        task.abort();
    }

    /// With the relay session down no release can reach the relay: none is sent, the role and
    /// the held fence stay, and the hub keeps running.
    #[tokio::test(start_paused = true)]
    async fn a_move_release_with_the_relay_session_down_changes_nothing() {
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        relay.capability.send_replace(None);
        // The session close is handled before the command (biased select).

        assert!(!handle.release_to("mach-b").await);

        assert_eq!(move_releases(&relay), 0, "no release without a session");
        assert_eq!(handle.state().role(), Role::Holder { fence: 7 });
        assert_eq!(*hub.events.lock(), vec![None]);
        // The session is back: the held fence is renewed, not re-acquired.
        let acquires = relay.acquires();
        relay.capability.send_replace(Some(true));
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert_eq!(relay.acquires(), acquires);
        assert!(relay.sent.lock().iter().any(|(_, r)| matches!(
            r,
            SupervisorLeaseRequest::SupervisorLeaseRenew(SupervisorLeaseRenew { fence: 7, .. })
        )));
        assert_eq!(handle.state().role(), Role::Holder { fence: 7 });
        task.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn a_standby_with_the_hub_stopped_does_not_auto_promote() {
        let relay = FakeRelay::new(Some(true));
        *relay.acquire.lock() = Ok(SupervisorLeaseOutcome::Refused);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let cfg = config(clock, Some("acct"), None);
        let checkpoints = Arc::clone(&cfg.checkpoints);
        checkpoints.set_local(Some(checkpoint('b', true)));
        let (handle, task) = spawn_supervisor_lease_client(cfg, Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| matches!(r, Role::Standby { holder_label: Some(_), .. })).await;
        assert!(relay.sent.lock().iter().any(|(_, r)| matches!(
            r,
            SupervisorLeaseRequest::SupervisorLeaseAcquire(SupervisorLeaseAcquire { checkpoint_id: Some(id), force: false, .. })
                if *id == "b".repeat(64)
        )), "a standby asks with its newest checkpoint, never forced");
        assert_eq!(
            checkpoints.block_reason().map(|block| block.code().to_owned()).as_deref(),
            Some("LEASE_HELD")
        );

        checkpoints.set_hub_stopped(true);
        let sent = relay.sent.lock().len();
        tokio::time::sleep(Duration::from_secs(lease::RENEW_SECS * 4)).await;
        assert_eq!(relay.sent.lock().len(), sent, "a stopped machine does not take part");
        // The person's own request still goes out.
        handle.acquire(false).await;
        assert_eq!(relay.sent.lock().len(), sent + 1);
        task.abort();
    }

    /// Auto-promotion asks the relay at the acquire cadence and acquires only when the
    /// checkpoint state allows it: a lease recording another checkpoint sends no acquire; the
    /// one this machine holds sends an unforced acquire carrying it.
    #[tokio::test(start_paused = true)]
    async fn a_standby_auto_acquires_only_when_its_checkpoint_matches_the_lease() {
        let relay = FakeRelay::new(Some(true));
        *relay.acquire.lock() = Ok(SupervisorLeaseOutcome::Refused);
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let cfg = config(clock, Some("acct"), None);
        let checkpoints = Arc::clone(&cfg.checkpoints);
        checkpoints.set_local(Some(checkpoint('c', true)));
        let (handle, task) = spawn_supervisor_lease_client(cfg, Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| matches!(r, Role::Standby { holder_machine_id: Some(_), .. })).await;
        let acquires = relay.acquires();

        // The holder's lease is free but records another checkpoint: no acquire goes out.
        *relay.query.lock() = Some((SupervisorLeaseOutcome::Released, Some("d".repeat(64))));
        tokio::time::sleep(Duration::from_secs(lease::RENEW_SECS * 3)).await;
        assert!(relay.queries() >= 2, "the standby asks the relay at its cadence");
        assert_eq!(relay.acquires(), acquires, "a stale checkpoint never promotes");
        assert!(!checkpoints.may_auto_acquire());
        assert_eq!(
            checkpoints.block_reason().map(|block| block.code().to_owned()).as_deref(),
            Some("CHECKPOINT_STALE")
        );
        assert!(matches!(handle.state().role(), Role::Standby { .. }));

        // It records this machine's checkpoint: the next tick acquires with it, unforced.
        *relay.query.lock() = Some((SupervisorLeaseOutcome::Released, Some("c".repeat(64))));
        *relay.acquire.lock() = Ok(SupervisorLeaseOutcome::Granted);
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;
        assert_eq!(relay.acquires(), acquires + 1);
        let sent = relay.sent.lock();
        let last_acquire = sent
            .iter()
            .rev()
            .find_map(|(_, r)| match r {
                SupervisorLeaseRequest::SupervisorLeaseAcquire(acquire) => Some(acquire.clone()),
                _ => None,
            })
            .expect("an acquire was sent");
        drop(sent);
        assert_eq!(last_acquire.checkpoint_id, Some("c".repeat(64)));
        assert!(!last_acquire.force, "auto-promotion is never forced");
        task.abort();
    }

    const STORE_USER: &str = "user-1";
    /// Relay clock at the [`StoreRelay`]'s creation.
    const STORE_T0: u64 = 1_000_000;

    /// A relay answering from the production lease rules ([`lease::acquire`], [`lease::renew`],
    /// [`lease::release`]) on a temporary account store enrolling `mach-a` and `mach-b`. Its
    /// clock is the paused Tokio clock in whole seconds from [`STORE_T0`]. The query outcome,
    /// the startup quarantine and the read-only standby's answers follow
    /// `remote::relay_server` (`query_outcome`, `quarantined_expiry`, `answer_standby_lease`).
    /// SQLite runs inline: a test fake on the paused current-thread runtime, where a blocking
    /// thread would let the runtime auto-advance the clock.
    struct StoreRelay {
        dir: tempfile::TempDir,
        start: Instant,
        quarantine_until: Mutex<u64>,
        read_only: Mutex<bool>,
    }

    impl StoreRelay {
        fn new() -> Arc<Self> {
            let dir = tempfile::tempdir().unwrap();
            let conn = crate::account::store::store_sqlite::open_sqlite_connection(dir.path()).unwrap();
            conn.execute("INSERT INTO users (user_id, email, created_at) VALUES (?1, 'o@example.com', 0)", [STORE_USER])
                .unwrap();
            for machine in ["mach-a", "mach-b"] {
                conn.execute(
                    "INSERT INTO machines (machine_record_id, user_id, machine_id, display_name, public_key, \
                     attach_public_key, relay_origin, platform, enrollment_epoch, enrolled_at) \
                     VALUES (?1, ?2, ?1, ?1, 'k', 'k', 'r', 'macos', 1, 0)",
                    rusqlite::params![machine, STORE_USER],
                )
                .unwrap();
            }
            drop(conn);
            Arc::new(Self { dir, start: Instant::now(), quarantine_until: Mutex::new(0), read_only: Mutex::new(false) })
        }

        fn now(&self) -> u64 {
            STORE_T0 + (Instant::now() - self.start).as_secs()
        }

        /// The test-clock instant at relay time `secs`.
        fn at(&self, secs: u64) -> Instant {
            self.start + Duration::from_secs(secs - STORE_T0)
        }

        fn row(&self) -> crate::account::store::SupervisorLeaseRecord {
            lease::current_in(self.dir.path(), STORE_USER).unwrap().expect("a lease row")
        }

        /// `machine` acquires directly at the relay (a holder this test runs no client for).
        fn seed(&self, machine: &str, checkpoint: Option<&str>) -> u64 {
            let now = self.now();
            lease::lease_transaction(self.dir.path(), |tx| lease::acquire(tx, STORE_USER, machine, now, checkpoint, false))
                .unwrap()
                .fence
        }

        /// A standby promotion: the epoch advances on the replica as it is (whatever it last
        /// recorded), and the relay starts writable with its startup quarantine. Returns the
        /// quarantine end.
        fn promote(&self) -> u64 {
            lease::lease_transaction(self.dir.path(), |tx| lease::bump_relay_epoch(tx)).unwrap();
            self.restart()
        }

        fn restart(&self) -> u64 {
            let end = self.now() + lease::TTL_SECS + lease::GRACE_SECS;
            *self.quarantine_until.lock() = end;
            end
        }

        /// The primary is down; the read-only standby answers.
        fn fail_over(&self) {
            *self.read_only.lock() = true;
        }

        /// The primary is back (same epoch) and restarts its quarantine.
        fn fail_back(&self) -> u64 {
            *self.read_only.lock() = false;
            self.restart()
        }

        fn link(self: &Arc<Self>, me: &'static str) -> Arc<StoreLink> {
            let (capability, _) = watch::channel(Some(true));
            Arc::new(StoreLink { relay: Arc::clone(self), me, capability, sent: Mutex::new(Vec::new()) })
        }

        fn answer(&self, me: &str, request: &SupervisorLeaseRequest) -> SupervisorLeaseAck {
            let now = self.now();
            let dir = self.dir.path();
            let row = lease::current_in(dir, STORE_USER).unwrap();
            let epoch = lease::lease_read(dir, lease::relay_epoch).unwrap();
            let read_only = *self.read_only.lock();
            let quarantined = !read_only
                && row.as_ref().is_some_and(|row| {
                    !row.released && row.holder_machine_id != me && now < *self.quarantine_until.lock()
                });
            let id = request.request_id();
            let describe = |outcome, reason: Option<&str>, row: Option<&crate::account::store::SupervisorLeaseRecord>| {
                SupervisorLeaseAck {
                    request_id: id.to_owned(),
                    outcome,
                    fence: row.map(|row| row.fence),
                    holder_machine_id: row.map(|row| row.holder_machine_id.clone()),
                    holder_label: row.map(|row| format!("Laptop {}", row.holder_machine_id)),
                    expires_in_secs: row.map(|row| row.expires_at.saturating_sub(now)),
                    previous_expires_in_secs: None,
                    checkpoint_id: row.and_then(|row| row.checkpoint_id.clone()),
                    account_machine_count: Some(2),
                    reason_code: reason.map(str::to_owned),
                    lease_capable_machines: None,
                }
            };
            let read_only_ack = || describe(SupervisorLeaseOutcome::Unavailable, Some("ACCOUNT_STORE_READ_ONLY"), None);
            match request {
                SupervisorLeaseRequest::SupervisorLeaseAcquire(_) | SupervisorLeaseRequest::SupervisorLeaseRelease(_)
                    if read_only =>
                {
                    read_only_ack()
                }
                SupervisorLeaseRequest::SupervisorLeaseAcquire(_) if quarantined => {
                    describe(SupervisorLeaseOutcome::Refused, Some("LEASE_HELD"), row.as_ref())
                }
                SupervisorLeaseRequest::SupervisorLeaseAcquire(acquire) => {
                    let checkpoint = acquire.checkpoint_id.as_deref();
                    match lease::lease_transaction(dir, |tx| {
                        lease::acquire(tx, STORE_USER, me, now, checkpoint, acquire.force)
                    }) {
                        Ok(granted) => describe(SupervisorLeaseOutcome::Granted, None, Some(&granted)),
                        Err(error) => describe(SupervisorLeaseOutcome::Refused, Some(error.code()), row.as_ref()),
                    }
                }
                SupervisorLeaseRequest::SupervisorLeaseRenew(renew) if read_only => {
                    let held = row.as_ref().filter(|row| {
                        !row.released
                            && row.holder_machine_id == me
                            && row.fence == renew.fence
                            && lease::fence_epoch(renew.fence) == epoch
                    });
                    match held {
                        Some(row) if now + lease::TTL_SECS <= row.expires_at => {
                            describe(SupervisorLeaseOutcome::Renewed, None, Some(row))
                        }
                        Some(row) if now < row.expires_at => describe(
                            SupervisorLeaseOutcome::Unavailable,
                            Some("ACCOUNT_STORE_READ_ONLY"),
                            Some(row),
                        ),
                        _ => describe(SupervisorLeaseOutcome::Refused, Some("LEASE_NOT_HOLDER"), row.as_ref()),
                    }
                }
                SupervisorLeaseRequest::SupervisorLeaseRenew(renew) => {
                    let checkpoint = renew.checkpoint_id.as_deref();
                    match lease::lease_transaction(dir, |tx| lease::renew(tx, STORE_USER, me, renew.fence, now, checkpoint)) {
                        Ok(renewed) => describe(SupervisorLeaseOutcome::Renewed, None, Some(&renewed)),
                        Err(error) => describe(SupervisorLeaseOutcome::Refused, Some(error.code()), row.as_ref()),
                    }
                }
                SupervisorLeaseRequest::SupervisorLeaseRelease(release) => {
                    let checkpoint = release.checkpoint_id.as_deref();
                    lease::lease_transaction(dir, |tx| lease::release(tx, STORE_USER, me, release.fence, now, checkpoint))
                        .unwrap();
                    let row = lease::current_in(dir, STORE_USER).unwrap();
                    describe(SupervisorLeaseOutcome::Released, None, row.as_ref())
                }
                SupervisorLeaseRequest::SupervisorLeaseQuery(_) => {
                    let (outcome, reason) = match &row {
                        _ if quarantined => (SupervisorLeaseOutcome::Refused, Some("LEASE_HELD")),
                        Some(row)
                            if !row.released
                                && lease::fence_epoch(row.fence) == epoch
                                && now < row.expires_at + lease::GRACE_SECS =>
                        {
                            if row.holder_machine_id == me {
                                (SupervisorLeaseOutcome::Granted, None)
                            } else {
                                (SupervisorLeaseOutcome::Refused, Some("LEASE_HELD"))
                            }
                        }
                        _ => (SupervisorLeaseOutcome::Released, None),
                    };
                    describe(outcome, reason, row.as_ref())
                }
            }
        }
    }

    /// One machine's control session to a [`StoreRelay`].
    struct StoreLink {
        relay: Arc<StoreRelay>,
        me: &'static str,
        capability: watch::Sender<Option<bool>>,
        sent: Mutex<Vec<SupervisorLeaseRequest>>,
    }

    impl StoreLink {
        fn acquires(&self) -> Vec<SupervisorLeaseAcquire> {
            self.sent
                .lock()
                .iter()
                .filter_map(|r| match r {
                    SupervisorLeaseRequest::SupervisorLeaseAcquire(acquire) => Some(acquire.clone()),
                    _ => None,
                })
                .collect()
        }

        fn queries(&self) -> usize {
            self.sent.lock().iter().filter(|r| matches!(r, SupervisorLeaseRequest::SupervisorLeaseQuery(_))).count()
        }
    }

    #[async_trait::async_trait]
    impl LeaseTransport for StoreLink {
        fn subscribe_capability(&self) -> watch::Receiver<Option<bool>> {
            self.capability.subscribe()
        }

        async fn call(&self, request: SupervisorLeaseRequest) -> Result<SupervisorLeaseAck, LeaseCallError> {
            let ack = self.relay.answer(self.me, &request);
            self.sent.lock().push(request);
            Ok(ack)
        }
    }

    fn names_holder(role: &Role, machine: &str) -> bool {
        matches!(role, Role::Standby { holder_machine_id: Some(holder), .. } if holder == machine)
    }

    fn held_fence(role: &Role) -> Option<u64> {
        match role {
            Role::Holder { fence } if *fence > 0 => Some(*fence),
            _ => None,
        }
    }

    /// Machine `machine`'s client on `relay` with `local` as its newest checkpoint.
    async fn store_client(
        relay: &Arc<StoreRelay>,
        machine: &'static str,
        local: crate::supervisor_checkpoint::StoredCheckpoint,
    ) -> (SupervisorLeaseHandle, tokio::task::JoinHandle<()>, Arc<StoreLink>, Arc<crate::supervisor_checkpoint::CheckpointState>) {
        let (clock, _) = test_clock();
        let mut cfg = config(clock, Some("acct"), None);
        cfg.machine_id = machine.into();
        cfg.checkpoints.set_local(Some(local));
        let checkpoints = Arc::clone(&cfg.checkpoints);
        let link = relay.link(machine);
        let (handle, task) = spawn_supervisor_lease_client(cfg, Arc::clone(&link), Arc::new(FakeHub::default())).await;
        (handle, task, link, checkpoints)
    }

    /// Lagging-replica promotion: A holds with checkpoint `o` recorded and has since reported
    /// `n` to the old primary; the promoted replica predates that report. A's renew of its
    /// old-epoch fence is refused (`LEASE_NOT_HOLDER`) and A is standby naming itself; its
    /// query finds its own row free with the lagging `o`. A auto-acquires (unforced, epoch 1)
    /// inside the quarantine, so B, a standby holding exactly `o`, is never granted.
    #[tokio::test(start_paused = true)]
    async fn after_a_lagging_replica_promotion_the_old_holder_reacquires_and_the_lagging_standby_does_not() {
        let relay = StoreRelay::new();
        let old = checkpoint('o', true);
        let newer = checkpoint('n', false);
        let (a, a_task, a_link, a_checkpoints) = store_client(&relay, "mach-a", old.clone()).await;
        wait_role(&a, |r| held_fence(r).is_some()).await;
        assert_eq!(relay.row().checkpoint_id.as_deref(), Some(old.id.as_str()));
        // `n` reached the old primary; the replica restored for the promotion lacks it.
        a_checkpoints.publish(newer.clone());
        a_checkpoints.mark_reported(&newer.id);
        let (b, b_task, b_link, b_checkpoints) = store_client(&relay, "mach-b", old.clone()).await;
        wait_role(&b, |r| names_holder(r, "mach-a")).await;
        assert_eq!(b_link.acquires().len(), 1, "B's first acquire, refused while A holds");

        let quarantine_end = relay.promote();
        wait_role(&a, |r| names_holder(r, "mach-a")).await;
        let fence = held_fence(&wait_role(&a, |r| held_fence(r).is_some()).await).unwrap();
        assert!(relay.now() < quarantine_end, "A re-acquired inside the quarantine");
        assert_eq!(lease::fence_epoch(fence), 1);
        let last = a_link.acquires().pop().expect("A acquired");
        assert_eq!((last.checkpoint_id.as_deref(), last.force), (Some(newer.id.as_str()), false));
        let row = relay.row();
        assert_eq!((row.holder_machine_id.as_str(), row.fence), ("mach-a", fence));
        assert_eq!(row.checkpoint_id.as_deref(), Some(newer.id.as_str()));

        // Well past the quarantine: A renews on, B only ever saw the lease held.
        tokio::time::sleep_until(relay.at(quarantine_end) + Duration::from_secs(lease::RENEW_SECS * 3)).await;
        assert_eq!(a.state().role(), Role::Holder { fence });
        assert_eq!(relay.row().holder_machine_id, "mach-a");
        assert_eq!(b_link.acquires().len(), 1, "B never asked for the lease again");
        assert!(b_link.queries() >= 2, "B kept asking the relay at its cadence");
        assert!(names_holder(&b.state().role(), "mach-a"));
        assert_eq!(b_checkpoints.block_reason().map(|block| block.code().to_owned()).as_deref(), Some("LEASE_HELD"));
        a_task.abort();
        b_task.abort();
    }

    /// Same epoch, past grace: A published `n` while the primary was down; the read-only
    /// standby never records a renew's checkpoint and never extends the lease, so A stands
    /// down and is told it no longer holds. After failback the row is free past grace, names
    /// A and records the older `o`: A auto-acquires its own row (same fence) recording `n`.
    #[tokio::test(start_paused = true)]
    async fn a_holder_lapsed_past_grace_with_an_unreported_checkpoint_reacquires_its_own_row() {
        let relay = StoreRelay::new();
        let old = checkpoint('o', true);
        let newer = checkpoint('n', false);
        let (a, a_task, a_link, a_checkpoints) = store_client(&relay, "mach-a", old.clone()).await;
        let fence = held_fence(&wait_role(&a, |r| held_fence(r).is_some()).await).unwrap();
        relay.fail_over();
        a_checkpoints.publish(newer.clone());
        let expires_at = relay.row().expires_at;

        wait_role(&a, |r| names_holder(r, "mach-a")).await;
        tokio::time::sleep_until(relay.at(expires_at + lease::GRACE_SECS)).await;
        assert_eq!(relay.row().checkpoint_id.as_deref(), Some(old.id.as_str()), "the standby recorded nothing");
        assert_eq!(a_checkpoints.unreported().as_deref(), Some(newer.id.as_str()));
        relay.fail_back();

        wait_role(&a, |r| *r == Role::Holder { fence }).await;
        let row = relay.row();
        assert_eq!((row.holder_machine_id.as_str(), row.fence), ("mach-a", fence), "same epoch: the same fence");
        assert!(relay.now() >= expires_at + lease::GRACE_SECS, "taken past grace");
        assert_eq!(row.checkpoint_id.as_deref(), Some(newer.id.as_str()));
        let last = a_link.acquires().pop().expect("A acquired");
        assert!(!last.force, "auto-promotion is never forced");
        a_task.abort();
    }

    /// The own-row exemption is only for the row's holder: B, whose newest checkpoint is not
    /// the one A's free row records, never auto-acquires; with the recorded one it does.
    #[tokio::test(start_paused = true)]
    async fn another_machine_with_a_mismatched_checkpoint_still_never_auto_acquires() {
        let relay = StoreRelay::new();
        let old = checkpoint('o', true);
        let fence = relay.seed("mach-a", Some(old.id.as_str()));
        let (b, b_task, b_link, b_checkpoints) = store_client(&relay, "mach-b", checkpoint('x', true)).await;
        wait_role(&b, |r| names_holder(r, "mach-a")).await;
        let expires_at = relay.row().expires_at;

        tokio::time::sleep_until(relay.at(expires_at + lease::GRACE_SECS) + Duration::from_secs(lease::RENEW_SECS * 3)).await;
        assert_eq!(b_link.acquires().len(), 1, "only B's first acquire, refused while A held");
        assert!(b_link.queries() >= 2);
        assert!(!b_checkpoints.may_auto_acquire());
        assert_eq!(
            b_checkpoints.block_reason().map(|block| block.code().to_owned()).as_deref(),
            Some("CHECKPOINT_STALE")
        );
        assert_eq!((relay.row().holder_machine_id.as_str(), relay.row().fence), ("mach-a", fence));

        b_checkpoints.set_local(Some(old.clone()));
        let taken = held_fence(&wait_role(&b, |r| held_fence(r).is_some()).await).unwrap();
        assert!(taken > fence);
        assert_eq!(relay.row().holder_machine_id, "mach-b");
        b_task.abort();
    }

    async fn a_stale_fence_stand_down_case() {
        use crate::agent_control::fence::SupervisorFenceSource;
        let relay = FakeRelay::new(Some(true));
        let hub = Arc::new(FakeHub::default());
        let (clock, _) = test_clock();
        let (handle, task) =
            spawn_supervisor_lease_client(config(clock, Some("acct"), None), Arc::clone(&relay), Arc::clone(&hub)).await;
        wait_role(&handle, |r| *r == Role::Holder { fence: 7 }).await;

        SupervisorFenceSource::stand_down(handle.state());

        wait_role(&handle, |r| *r == Role::standby_unknown()).await;
        // Several renew periods later (paused time): nothing renewed, re-acquired or restarted.
        tokio::time::sleep(VALID_FOR * 3).await;
        assert_eq!(handle.state().role(), Role::standby_unknown());
        assert!(!handle.is_holder_now());
        assert_eq!(handle.current_fence(), 0);
        let sent = relay.sent.lock();
        let released = sent
            .iter()
            .position(|(_, r)| {
                matches!(r, SupervisorLeaseRequest::SupervisorLeaseRelease(SupervisorLeaseRelease { fence: 7, .. }))
            })
            .expect("the stale fence was released");
        assert!(
            sent[released + 1..].iter().all(|(_, r)| matches!(r, SupervisorLeaseRequest::SupervisorLeaseQuery(_))),
            "no renew or acquire after the release: {:?}",
            &sent[released..]
        );
        drop(sent);
        let events = hub.events.lock();
        let stopped = events
            .iter()
            .position(|e| *e == Some(StandDownReason::Released))
            .expect("the hub was stood down by the release");
        assert!(events[stopped..].iter().all(Option::is_some), "the hub was never started again");
        drop(events);
        task.abort();
    }
}
