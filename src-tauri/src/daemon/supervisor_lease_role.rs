//! Pure supervisor lease role machine (plan todo 3, part A).
//!
//! No I/O, no tokio and no clock reads inside the transition logic: every
//! entry point takes a [`Now`] (monotonic + wall clock) from the caller, so
//! the rules are unit-testable with explicit time.
//!
//! # Public API (consumed by part B: relay I/O + wiring)
//! - [`RoleMachine::new`]`(cfg, self_machine_id, account_bound, persisted, now)`
//!   at boot. `account_bound` = an enrollment record exists (account mode);
//!   `persisted` = parsed `<data>/supervisor-lease.json` (None if missing).
//!   Returns the machine plus the initial [`Action`]s. A persisted holder fence
//!   is renewed first on the next capable session (refusal => Standby, then
//!   acquire at the normal cadence).
//! - [`RoleMachine::handle`]`(event, now) -> Vec<Action>` for every [`Event`]:
//!   relay session opened (with whether `supervisorLeaseV1` was advertised) or
//!   closed, relay unreachable at boot, acquire/renew acks mapped with
//!   [`LeaseAck::from_wire`] together with the SEND time of the request, periodic
//!   `Tick`s (about once a second), and `AcquireRequested{force}`,
//!   `ReleaseRequested`, `CapabilityReset` from the CLI/UI.
//! - Execute the returned [`Action`]s in order: start/stop the hub, send
//!   acquire/renew/release over the control session (remember the send time
//!   and pass it back in the ack event), persist the snapshot (0600, atomic
//!   write, off the Tokio worker threads).
//! - [`RoleMachine::is_holder_now`] before every side effect (strict Holder);
//!   [`RoleMachine::may_act_now`] additionally allows Provisional.
//!   [`RoleMachine::role`] / [`RoleMachine::fence`] for status and fencing.
//!
//! # Rules
//! - Account-less/selfhost, or account mode whose relay has never advertised
//!   the capability: Holder with fence 0, no deadline (today's behavior).
//! - Capability latch: once seen, `lease_capable` is persisted; a later session
//!   without it yields `Unleased{RELAY_LEASE_UNSUPPORTED}` (hub stopped), never
//!   Holder. Only `CapabilityReset` clears the latch.
//! - A grant sets deadline = request SEND time + TTL - 15s margin, on both the
//!   monotonic and the wall clock; Holder only while neither has passed.
//!   Reaching it without renew => Standby{unknown} + `StopHub{DeadlineReached}`.
//! - Wall/monotonic skew between two observations larger than
//!   RENEW + 5s (sleep/wake, clock change) => Standby{unknown} immediately and a
//!   renew with the old fence; Holder again only when a renew succeeds.
//! - Renew every RENEW while Holder. Standby/Provisional retry acquire at the
//!   same cadence (never a tight loop, also for `LEASE_CHECKPOINT_MISMATCH`);
//!   after an explicit release, auto-acquire pauses until `AcquireRequested`,
//!   and grants for requests sent before the release are ignored.
//! - Any refusal => Standby with the reported holder. `Unavailable` with
//!   `LEASE_UNSUPPORTED` => Unleased; other `Unavailable` codes change nothing.
//! - A grant whose deadline has already passed on arrival does not restore
//!   Holder; the fence is renewed again on the next attempt.
//! - Provisional is not affected by clock jumps (it has no deadline).
//! - Boot with relay unreachable (capability latched): Provisional only if the
//!   persisted last_role == holder AND account_machine_count == 1, else Standby.
//! - First capability sighting while Holder fence 0: Provisional (hub keeps
//!   running) until the acquire ack decides.

use std::time::{Duration, Instant, SystemTime};

use crate::account::supervisor_lease as lease;
use crate::remote::protocol::{SupervisorLeaseAck, SupervisorLeaseOutcome};

/// Reason code for a relay session that lacks the lease capability after the
/// latch was set.
pub const RELAY_LEASE_UNSUPPORTED: &str = "RELAY_LEASE_UNSUPPORTED";
/// Hint shown next to [`RELAY_LEASE_UNSUPPORTED`] in status output.
pub const RELAY_LEASE_UNSUPPORTED_HINT: &str =
    "relay no longer advertises supervisorLeaseV1; after a deliberate relay downgrade run 'ferryx hub lease reset-capability'";
/// Refusal code for a previous holder reacquiring without the matching
/// checkpoint; handled like any other refusal (Standby, normal cadence).
pub const LEASE_CHECKPOINT_MISMATCH: &str = "LEASE_CHECKPOINT_MISMATCH";
/// Local stand-down margin before the relay-side lease expiry.
pub const STAND_DOWN_MARGIN_SECS: u64 = 15;
/// Extra slack on top of RENEW before a wall/monotonic skew counts as a jump.
pub const WALL_JUMP_SLACK_SECS: u64 = 5;

/// A point in time on both clocks, supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    pub mono: Instant,
    pub wall: SystemTime,
}

impl Now {
    /// Reads both clocks (for part B; the machine itself never calls this).
    pub fn current() -> Self {
        Self {
            mono: Instant::now(),
            wall: SystemTime::now(),
        }
    }
}

/// Timing parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoleConfig {
    pub ttl: Duration,
    pub renew_every: Duration,
    pub margin: Duration,
}

impl RoleConfig {
    /// TTL and renew cadence in seconds with the standard 15s margin.
    pub fn from_secs(ttl_secs: u64, renew_secs: u64) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_secs),
            renew_every: Duration::from_secs(renew_secs),
            margin: Duration::from_secs(STAND_DOWN_MARGIN_SECS),
        }
    }

    fn valid_for(&self) -> Duration {
        self.ttl.saturating_sub(self.margin)
    }

    fn jump_threshold(&self) -> Duration {
        self.renew_every + Duration::from_secs(WALL_JUMP_SLACK_SECS)
    }
}

impl Default for RoleConfig {
    fn default() -> Self {
        Self::from_secs(lease::TTL_SECS, lease::RENEW_SECS)
    }
}

/// Hub role of this daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    /// Runs the hub. Fence 0 = unleased legacy mode (no deadline).
    Holder { fence: u64 },
    /// Runs the hub offline after a single-machine boot, lease not confirmed.
    Provisional,
    /// Hub stopped; another machine (or unknown) holds the lease.
    Standby {
        holder_machine_id: Option<String>,
        holder_label: Option<String>,
        fence: Option<u64>,
    },
    /// Hub stopped; relay cannot arbitrate (treated like Standby).
    Unleased { reason: String },
}

impl Role {
    pub fn standby_unknown() -> Self {
        Self::Standby {
            holder_machine_id: None,
            holder_label: None,
            fence: None,
        }
    }

    pub fn runs_hub(&self) -> bool {
        matches!(self, Self::Holder { .. } | Self::Provisional)
    }
}

/// Relay answer to an acquire or renew request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseAck {
    Granted {
        fence: u64,
        account_machine_count: Option<u32>,
    },
    Refused {
        reason_code: String,
        holder_machine_id: Option<String>,
        holder_label: Option<String>,
        fence: Option<u64>,
        account_machine_count: Option<u32>,
    },
    /// The relay could not arbitrate (`LEASE_UNSUPPORTED`, read-only store, ...).
    Unavailable { reason_code: String },
    /// Answer to a release request; no role effect.
    Released,
}

impl LeaseAck {
    /// Maps the wire ack. `Granted` and `Renewed` both become [`LeaseAck::Granted`];
    /// a grant without a positive fence is treated as `Unavailable`.
    pub fn from_wire(ack: &SupervisorLeaseAck) -> Self {
        match ack.outcome {
            SupervisorLeaseOutcome::Granted | SupervisorLeaseOutcome::Renewed => match ack.fence {
                Some(fence) if fence > 0 => LeaseAck::Granted {
                    fence,
                    account_machine_count: ack.account_machine_count,
                },
                _ => LeaseAck::Unavailable {
                    reason_code: LEASE_ACK_MISSING_FENCE.to_string(),
                },
            },
            SupervisorLeaseOutcome::Refused => LeaseAck::Refused {
                reason_code: ack
                    .reason_code
                    .clone()
                    .unwrap_or_else(|| LEASE_REFUSED.to_string()),
                holder_machine_id: ack.holder_machine_id.clone(),
                holder_label: ack.holder_label.clone(),
                fence: ack.fence,
                account_machine_count: ack.account_machine_count,
            },
            SupervisorLeaseOutcome::Unavailable => LeaseAck::Unavailable {
                reason_code: ack
                    .reason_code
                    .clone()
                    .unwrap_or_else(|| lease::LEASE_UNSUPPORTED.to_string()),
            },
            SupervisorLeaseOutcome::Released => LeaseAck::Released,
        }
    }
}

/// Local code for a grant ack that carried no usable fence.
pub const LEASE_ACK_MISSING_FENCE: &str = "LEASE_ACK_MISSING_FENCE";
/// Local code for a refusal without a reason code.
pub const LEASE_REFUSED: &str = "LEASE_REFUSED";

/// Inputs to [`RoleMachine::handle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The first relay connection attempt at boot failed.
    RelayUnreachableAtBoot,
    /// A relay control session is up; `lease_capable` = it advertised
    /// `supervisorLeaseV1`.
    SessionOpened { lease_capable: bool },
    SessionClosed,
    AcquireAck { sent_at: Now, ack: LeaseAck },
    RenewAck { sent_at: Now, ack: LeaseAck },
    Tick,
    AcquireRequested { force: bool },
    ReleaseRequested,
    /// `ferryx hub lease reset-capability`: clears the capability latch.
    CapabilityReset,
}

/// Why the hub is being stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandDownReason {
    DeadlineReached,
    WallClockJump,
    Refused,
    Released,
    RelayLeaseUnsupported,
}

/// Outputs of the machine, to be executed in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    StartHub,
    /// Stop the hub; this is the stand-down event.
    StopHub { reason: StandDownReason },
    SendAcquire { force: bool },
    SendRenew { fence: u64 },
    SendRelease { fence: u64 },
    Persist(PersistedLease),
}

/// Last confirmed role kept on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LastRole {
    Holder,
    Standby,
    Unleased,
}

/// Contents of `<data>/supervisor-lease.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PersistedLease {
    pub last_role: LastRole,
    pub last_fence: u64,
    #[serde(default)]
    pub account_machine_count: Option<u32>,
    #[serde(default)]
    pub holder_machine_id: Option<String>,
    #[serde(default)]
    pub lease_capable: bool,
}

impl Default for PersistedLease {
    fn default() -> Self {
        Self {
            last_role: LastRole::Standby,
            last_fence: 0,
            account_machine_count: None,
            holder_machine_id: None,
            lease_capable: false,
        }
    }
}

/// Absolute difference between the wall-clock and the monotonic advance from
/// `prev` to `now` (sleep/wake or a clock change shows up here).
fn clock_skew(prev: Now, now: Now) -> Duration {
    let mono = now.mono.saturating_duration_since(prev.mono);
    match now.wall.duration_since(prev.wall) {
        Ok(wall) if wall >= mono => wall - mono,
        Ok(wall) => mono - wall,
        Err(backwards) => mono + backwards.duration(),
    }
}

fn passed(deadline: Now, now: Now) -> bool {
    now.mono >= deadline.mono || now.wall >= deadline.wall
}

/// The pure role machine. See the module docs for the rules.
#[derive(Debug, Clone)]
pub struct RoleMachine {
    cfg: RoleConfig,
    self_machine_id: String,
    account_bound: bool,
    snapshot: PersistedLease,
    role: Role,
    /// Local stand-down deadline of the current Holder grant (both clocks).
    deadline: Option<Now>,
    /// Fence to renew (the last granted one), cleared by a refusal or release.
    held_fence: Option<u64>,
    last_obs: Now,
    next_attempt: Option<Instant>,
    session_open: bool,
    session_capable: bool,
    seen_session: bool,
    auto_acquire: bool,
    released_at: Option<Instant>,
    /// Send time of the request behind the newest grant; older refusals are stale.
    last_grant_sent: Option<Instant>,
}

impl RoleMachine {
    pub fn new(
        cfg: RoleConfig,
        self_machine_id: impl Into<String>,
        account_bound: bool,
        persisted: Option<PersistedLease>,
        now: Now,
    ) -> (Self, Vec<Action>) {
        let snapshot = persisted.unwrap_or_default();
        let leased = account_bound && snapshot.lease_capable;
        let held_fence = (leased
            && snapshot.last_role == LastRole::Holder
            && snapshot.last_fence > 0)
            .then_some(snapshot.last_fence);
        let role = if leased {
            Role::standby_unknown()
        } else {
            Role::Holder { fence: 0 }
        };
        let actions = if leased { Vec::new() } else { vec![Action::StartHub] };
        let machine = Self {
            cfg,
            self_machine_id: self_machine_id.into(),
            account_bound,
            snapshot,
            role,
            deadline: None,
            held_fence,
            last_obs: now,
            next_attempt: None,
            session_open: false,
            session_capable: false,
            seen_session: false,
            auto_acquire: true,
            released_at: None,
            last_grant_sent: None,
        };
        (machine, actions)
    }

    pub fn role(&self) -> &Role {
        &self.role
    }

    /// Fence to stamp on side effects: `Some` only while Holder (0 = unleased mode).
    pub fn fence(&self) -> Option<u64> {
        match self.role {
            Role::Holder { fence } => Some(fence),
            _ => None,
        }
    }

    /// Last persisted snapshot (what `supervisor-lease.json` should contain).
    pub fn snapshot(&self) -> &PersistedLease {
        &self.snapshot
    }

    /// True when the relay arbitrates the role (account mode + capability latch).
    pub fn leased(&self) -> bool {
        self.account_bound && self.snapshot.lease_capable
    }

    /// Status hint for the current role, if any.
    pub fn status_hint(&self) -> Option<&'static str> {
        match &self.role {
            Role::Unleased { reason } if reason == RELAY_LEASE_UNSUPPORTED => {
                Some(RELAY_LEASE_UNSUPPORTED_HINT)
            }
            _ => None,
        }
    }

    /// Strict Holder check at the moment of use: the deadline must not have
    /// passed on either clock and no clock jump may have happened since the
    /// last observation. Fence 0 (unleased mode) is always true.
    pub fn is_holder_now(&self, now: Now) -> bool {
        match self.role {
            Role::Holder { fence: 0 } => true,
            Role::Holder { .. } => {
                clock_skew(self.last_obs, now) <= self.cfg.jump_threshold()
                    && self.deadline.is_some_and(|deadline| !passed(deadline, now))
            }
            _ => false,
        }
    }

    /// [`Self::is_holder_now`] or Provisional (offline single-machine hub).
    pub fn may_act_now(&self, now: Now) -> bool {
        self.role == Role::Provisional || self.is_holder_now(now)
    }

    pub fn handle(&mut self, event: Event, now: Now) -> Vec<Action> {
        let mut out = Vec::new();
        self.observe_clocks(now, &mut out);
        self.check_deadline(now, &mut out);
        match event {
            Event::RelayUnreachableAtBoot => self.on_unreachable_at_boot(&mut out),
            Event::SessionOpened { lease_capable } => {
                self.on_session_opened(lease_capable, now, &mut out)
            }
            Event::SessionClosed => {
                self.session_open = false;
                self.session_capable = false;
            }
            Event::AcquireAck { sent_at, ack } | Event::RenewAck { sent_at, ack } => {
                self.on_ack(sent_at, ack, now, &mut out)
            }
            Event::Tick => {
                if self.leased()
                    && !matches!(self.role, Role::Unleased { .. })
                    && self.next_attempt.is_none_or(|at| now.mono >= at)
                {
                    self.attempt(now, false, &mut out);
                }
            }
            Event::AcquireRequested { force } => {
                if self.leased() {
                    self.auto_acquire = true;
                    self.attempt(now, force, &mut out);
                }
            }
            Event::ReleaseRequested => self.on_release(now, &mut out),
            Event::CapabilityReset => self.on_capability_reset(now, &mut out),
        }
        out
    }

    fn can_send(&self) -> bool {
        self.session_open && self.session_capable
    }

    fn set_snapshot(&mut self, snapshot: PersistedLease, out: &mut Vec<Action>) {
        if snapshot != self.snapshot {
            self.snapshot = snapshot;
            out.push(Action::Persist(self.snapshot.clone()));
        }
    }

    fn stand_down(&mut self, role: Role, reason: StandDownReason, out: &mut Vec<Action>) {
        let was_running = self.role.runs_hub();
        self.role = role;
        self.deadline = None;
        if was_running {
            out.push(Action::StopHub { reason });
        }
    }

    fn observe_clocks(&mut self, now: Now, out: &mut Vec<Action>) {
        let jumped = clock_skew(self.last_obs, now) > self.cfg.jump_threshold();
        self.last_obs = now;
        if !jumped || !matches!(self.role, Role::Holder { fence } if fence > 0) {
            return;
        }
        self.stand_down(Role::standby_unknown(), StandDownReason::WallClockJump, out);
        if let (Some(fence), true) = (self.held_fence, self.can_send()) {
            out.push(Action::SendRenew { fence });
            self.next_attempt = Some(now.mono + self.cfg.renew_every);
        }
    }

    fn check_deadline(&mut self, now: Now, out: &mut Vec<Action>) {
        let expired = match self.role {
            Role::Holder { fence } if fence > 0 => {
                self.deadline.is_none_or(|deadline| passed(deadline, now))
            }
            _ => false,
        };
        if expired {
            self.stand_down(Role::standby_unknown(), StandDownReason::DeadlineReached, out);
        }
    }

    fn attempt(&mut self, now: Now, force: bool, out: &mut Vec<Action>) {
        if !self.can_send() {
            return;
        }
        match self.held_fence {
            Some(fence) if !force => out.push(Action::SendRenew { fence }),
            _ if self.auto_acquire || force => out.push(Action::SendAcquire { force }),
            _ => return,
        }
        self.next_attempt = Some(now.mono + self.cfg.renew_every);
    }

    fn on_unreachable_at_boot(&mut self, out: &mut Vec<Action>) {
        if !self.leased() || self.seen_session || self.role != Role::standby_unknown() {
            return;
        }
        if self.snapshot.last_role == LastRole::Holder
            && self.snapshot.account_machine_count == Some(1)
        {
            self.role = Role::Provisional;
            out.push(Action::StartHub);
        }
    }

    fn on_session_opened(&mut self, lease_capable: bool, now: Now, out: &mut Vec<Action>) {
        self.session_open = true;
        self.session_capable = lease_capable;
        self.seen_session = true;
        if !self.account_bound {
            return;
        }
        if !lease_capable {
            if self.snapshot.lease_capable {
                let role = Role::Unleased {
                    reason: RELAY_LEASE_UNSUPPORTED.to_string(),
                };
                self.stand_down(role, StandDownReason::RelayLeaseUnsupported, out);
            }
            return;
        }
        if !self.snapshot.lease_capable {
            let snapshot = PersistedLease {
                lease_capable: true,
                ..self.snapshot.clone()
            };
            self.set_snapshot(snapshot, out);
            if self.role == (Role::Holder { fence: 0 }) {
                self.role = Role::Provisional;
            }
        } else if matches!(self.role, Role::Unleased { .. }) {
            self.role = Role::standby_unknown();
        }
        self.next_attempt = None;
        self.attempt(now, false, out);
    }

    fn on_ack(&mut self, sent_at: Now, ack: LeaseAck, now: Now, out: &mut Vec<Action>) {
        if !self.leased() || matches!(self.role, Role::Unleased { .. }) {
            return;
        }
        let stale = |at: Option<Instant>| at.is_some_and(|at| sent_at.mono < at);
        match ack {
            LeaseAck::Granted {
                fence,
                account_machine_count,
            } => {
                if stale(self.released_at) {
                    return;
                }
                let deadline = Now {
                    mono: sent_at.mono + self.cfg.valid_for(),
                    wall: sent_at.wall + self.cfg.valid_for(),
                };
                self.held_fence = Some(fence);
                if passed(deadline, now) {
                    return;
                }
                if self.role == (Role::Holder { fence })
                    && self
                        .deadline
                        .is_some_and(|current| current.mono >= deadline.mono)
                {
                    return;
                }
                let was_running = self.role.runs_hub();
                self.role = Role::Holder { fence };
                self.deadline = Some(deadline);
                self.last_grant_sent = Some(sent_at.mono);
                if !was_running {
                    out.push(Action::StartHub);
                }
                let snapshot = PersistedLease {
                    last_role: LastRole::Holder,
                    last_fence: fence,
                    account_machine_count: account_machine_count
                        .or(self.snapshot.account_machine_count),
                    holder_machine_id: Some(self.self_machine_id.clone()),
                    lease_capable: true,
                };
                self.set_snapshot(snapshot, out);
            }
            LeaseAck::Refused {
                reason_code: _,
                holder_machine_id,
                holder_label,
                fence,
                account_machine_count,
            } => {
                if stale(self.last_grant_sent) {
                    return;
                }
                self.held_fence = None;
                let snapshot = PersistedLease {
                    last_role: LastRole::Standby,
                    last_fence: fence.unwrap_or(self.snapshot.last_fence),
                    account_machine_count: account_machine_count
                        .or(self.snapshot.account_machine_count),
                    holder_machine_id: holder_machine_id.clone(),
                    lease_capable: true,
                };
                let role = Role::Standby {
                    holder_machine_id,
                    holder_label,
                    fence,
                };
                self.stand_down(role, StandDownReason::Refused, out);
                self.set_snapshot(snapshot, out);
            }
            LeaseAck::Unavailable { reason_code } => {
                if reason_code == lease::LEASE_UNSUPPORTED {
                    let role = Role::Unleased {
                        reason: RELAY_LEASE_UNSUPPORTED.to_string(),
                    };
                    self.stand_down(role, StandDownReason::RelayLeaseUnsupported, out);
                }
            }
            LeaseAck::Released => {}
        }
    }

    fn on_release(&mut self, now: Now, out: &mut Vec<Action>) {
        if !self.leased() {
            return;
        }
        self.auto_acquire = false;
        self.released_at = Some(now.mono);
        if let (Some(fence), true) = (self.held_fence.take(), self.can_send()) {
            out.push(Action::SendRelease { fence });
        }
        if self.role.runs_hub() {
            self.stand_down(Role::standby_unknown(), StandDownReason::Released, out);
        }
        let snapshot = PersistedLease {
            last_role: LastRole::Standby,
            holder_machine_id: None,
            ..self.snapshot.clone()
        };
        self.set_snapshot(snapshot, out);
    }

    fn on_capability_reset(&mut self, now: Now, out: &mut Vec<Action>) {
        let snapshot = PersistedLease {
            last_role: LastRole::Holder,
            last_fence: 0,
            holder_machine_id: None,
            lease_capable: false,
            ..self.snapshot.clone()
        };
        self.set_snapshot(snapshot, out);
        self.held_fence = None;
        self.deadline = None;
        self.last_grant_sent = None;
        if !self.role.runs_hub() {
            out.push(Action::StartHub);
        }
        self.role = Role::Holder { fence: 0 };
        if self.can_send() {
            self.on_session_opened(true, now, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL_LESS_MARGIN: u64 = lease::TTL_SECS - STAND_DOWN_MARGIN_SECS;

    struct Clock {
        base: Now,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                base: Now {
                    mono: Instant::now(),
                    wall: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
                },
            }
        }

        /// Both clocks advanced by `secs`.
        fn at(&self, secs: u64) -> Now {
            self.skewed(secs, secs)
        }

        /// Monotonic advanced by `mono`, wall clock by `wall` seconds.
        fn skewed(&self, mono: u64, wall: u64) -> Now {
            Now {
                mono: self.base.mono + Duration::from_secs(mono),
                wall: self.base.wall + Duration::from_secs(wall),
            }
        }
    }

    fn latched(last_role: LastRole, fence: u64, count: Option<u32>) -> PersistedLease {
        PersistedLease {
            last_role,
            last_fence: fence,
            account_machine_count: count,
            holder_machine_id: None,
            lease_capable: true,
        }
    }

    fn granted(fence: u64) -> LeaseAck {
        LeaseAck::Granted {
            fence,
            account_machine_count: Some(2),
        }
    }

    fn refused(code: &str) -> LeaseAck {
        LeaseAck::Refused {
            reason_code: code.to_string(),
            holder_machine_id: Some("mach-b".into()),
            holder_label: Some("Studio".into()),
            fence: Some(7),
            account_machine_count: Some(2),
        }
    }

    fn has(actions: &[Action], wanted: &Action) -> bool {
        actions.iter().any(|action| action == wanted)
    }

    /// Latched standby machine with a capable session open at t=0 (acquire sent).
    fn standby(clock: &Clock) -> RoleMachine {
        let (mut m, out) = RoleMachine::new(
            RoleConfig::default(),
            "mach-a",
            true,
            Some(latched(LastRole::Standby, 0, Some(2))),
            clock.at(0),
        );
        assert!(out.is_empty());
        let open = m.handle(Event::SessionOpened { lease_capable: true }, clock.at(0));
        assert_eq!(open, vec![Action::SendAcquire { force: false }]);
        m
    }

    /// [`standby`] plus a grant of fence 5 for the acquire sent at t=0.
    fn holder(clock: &Clock) -> RoleMachine {
        let mut m = standby(clock);
        let out = m.handle(
            Event::AcquireAck {
                sent_at: clock.at(0),
                ack: granted(5),
            },
            clock.at(0),
        );
        assert!(has(&out, &Action::StartHub));
        assert_eq!(m.role(), &Role::Holder { fence: 5 });
        m
    }

    #[test]
    fn selfhost_is_holder_fence_zero() {
        let clock = Clock::new();
        let (mut m, out) =
            RoleMachine::new(RoleConfig::default(), "mach-a", false, None, clock.at(0));
        assert_eq!(out, vec![Action::StartHub]);
        assert_eq!(m.fence(), Some(0));
        let opened = m.handle(Event::SessionOpened { lease_capable: true }, clock.at(1));
        assert!(opened.is_empty());
        assert!(m.handle(Event::Tick, clock.at(10_000)).is_empty());
        assert!(m.is_holder_now(clock.skewed(10_000, 99_999)));
    }

    #[test]
    fn relay_without_capability_keeps_holder_fence_zero() {
        let clock = Clock::new();
        let (mut m, out) =
            RoleMachine::new(RoleConfig::default(), "mach-a", true, None, clock.at(0));
        assert_eq!(out, vec![Action::StartHub]);
        let opened = m.handle(Event::SessionOpened { lease_capable: false }, clock.at(1));
        assert!(opened.is_empty());
        assert!(m.handle(Event::Tick, clock.at(500)).is_empty());
        assert_eq!(m.role(), &Role::Holder { fence: 0 });
        assert!(m.is_holder_now(clock.at(500)));
    }

    #[test]
    fn first_capability_latches_and_acquires_without_stopping_the_hub() {
        let clock = Clock::new();
        let (mut m, _) =
            RoleMachine::new(RoleConfig::default(), "mach-a", true, None, clock.at(0));
        let out = m.handle(Event::SessionOpened { lease_capable: true }, clock.at(1));
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0], Action::Persist(p) if p.lease_capable));
        assert_eq!(out[1], Action::SendAcquire { force: false });
        assert_eq!(m.role(), &Role::Provisional);
        let out = m.handle(
            Event::AcquireAck {
                sent_at: clock.at(1),
                ack: granted(3),
            },
            clock.at(2),
        );
        assert!(!has(&out, &Action::StartHub));
        assert_eq!(m.role(), &Role::Holder { fence: 3 });
        assert_eq!(m.snapshot().last_role, LastRole::Holder);
        assert_eq!(m.snapshot().holder_machine_id.as_deref(), Some("mach-a"));
    }

    #[test]
    fn renew_keeps_holder() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        assert!(m.handle(Event::Tick, clock.at(19)).is_empty());
        let mut t = 20;
        while t <= 400 {
            let out = m.handle(Event::Tick, clock.at(t));
            assert_eq!(out, vec![Action::SendRenew { fence: 5 }], "t={t}");
            m.handle(
                Event::RenewAck {
                    sent_at: clock.at(t),
                    ack: granted(5),
                },
                clock.at(t + 1),
            );
            assert!(m.is_holder_now(clock.at(t + 1)));
            t += 20;
        }
        assert_eq!(m.role(), &Role::Holder { fence: 5 });
    }

    #[test]
    fn missed_renews_reach_standby_exactly_at_deadline() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        assert!(m.is_holder_now(clock.at(TTL_LESS_MARGIN - 1)));
        assert!(!m.is_holder_now(clock.at(TTL_LESS_MARGIN)));
        let out = m.handle(Event::Tick, clock.at(TTL_LESS_MARGIN - 1));
        assert_eq!(out, vec![Action::SendRenew { fence: 5 }]);
        assert_eq!(m.role(), &Role::Holder { fence: 5 });
        let out = m.handle(Event::Tick, clock.at(TTL_LESS_MARGIN));
        assert_eq!(
            out[0],
            Action::StopHub {
                reason: StandDownReason::DeadlineReached
            }
        );
        assert_eq!(m.role(), &Role::standby_unknown());
        assert_eq!(m.fence(), None);
    }

    #[test]
    fn deadline_is_measured_from_send_time() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::Tick, clock.at(20));
        // Ack delayed 10s: the deadline is still send(20) + TTL - margin.
        m.handle(
            Event::RenewAck {
                sent_at: clock.at(20),
                ack: granted(5),
            },
            clock.at(30),
        );
        assert!(m.is_holder_now(clock.at(20 + TTL_LESS_MARGIN - 1)));
        assert!(!m.is_holder_now(clock.at(20 + TTL_LESS_MARGIN)));
    }

    #[test]
    fn grant_arriving_after_its_deadline_does_not_restore_holder() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::Tick, clock.at(TTL_LESS_MARGIN));
        let out = m.handle(
            Event::RenewAck {
                sent_at: clock.at(0),
                ack: granted(5),
            },
            clock.at(TTL_LESS_MARGIN + 1),
        );
        assert!(out.is_empty());
        assert_eq!(m.role(), &Role::standby_unknown());
    }

    #[test]
    fn wall_clock_jump_drops_to_standby_before_any_work() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        let woke = clock.skewed(1, 3_600);
        assert!(!m.is_holder_now(woke));
        let out = m.handle(Event::Tick, woke);
        assert_eq!(
            out,
            vec![
                Action::StopHub {
                    reason: StandDownReason::WallClockJump
                },
                Action::SendRenew { fence: 5 },
            ]
        );
        assert_eq!(m.role(), &Role::standby_unknown());
        // Stays standby until a renew succeeds.
        assert!(m.handle(Event::Tick, clock.skewed(2, 3_601)).is_empty());
        let out = m.handle(
            Event::RenewAck {
                sent_at: woke,
                ack: granted(5),
            },
            clock.skewed(3, 3_602),
        );
        assert!(has(&out, &Action::StartHub));
        assert!(m.is_holder_now(clock.skewed(3, 3_602)));
    }

    #[test]
    fn backwards_wall_clock_is_a_jump() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        let back = Now {
            mono: clock.at(1).mono,
            wall: clock.at(0).wall - Duration::from_secs(3_600),
        };
        let out = m.handle(Event::Tick, back);
        assert!(has(
            &out,
            &Action::StopHub {
                reason: StandDownReason::WallClockJump
            }
        ));
    }

    #[test]
    fn small_clock_skew_is_not_a_jump() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        let out = m.handle(Event::Tick, clock.skewed(1, 1 + lease::RENEW_SECS + 5));
        assert!(out.is_empty());
        assert_eq!(m.role(), &Role::Holder { fence: 5 });
    }

    #[test]
    fn wall_clock_past_deadline_without_tick_is_not_holder() {
        let clock = Clock::new();
        let m = holder(&clock);
        // Monotonic did not move (sleep), wall clock is past the deadline, no tick yet.
        assert!(!m.is_holder_now(clock.skewed(0, TTL_LESS_MARGIN)));
        assert!(!m.may_act_now(clock.skewed(0, TTL_LESS_MARGIN)));
        assert!(m.is_holder_now(clock.at(1)));
    }

    #[test]
    fn refused_acquire_is_standby_with_holder_label() {
        let clock = Clock::new();
        let mut m = standby(&clock);
        let out = m.handle(
            Event::AcquireAck {
                sent_at: clock.at(0),
                ack: refused("LEASE_HELD"),
            },
            clock.at(1),
        );
        assert!(!out
            .iter()
            .any(|a| matches!(a, Action::StopHub { .. } | Action::StartHub)));
        assert_eq!(
            m.role(),
            &Role::Standby {
                holder_machine_id: Some("mach-b".into()),
                holder_label: Some("Studio".into()),
                fence: Some(7),
            }
        );
        assert_eq!(m.snapshot().holder_machine_id.as_deref(), Some("mach-b"));
        assert_eq!(m.snapshot().last_role, LastRole::Standby);
    }

    #[test]
    fn refused_renew_stops_the_hub_and_next_attempt_acquires() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::Tick, clock.at(20));
        let out = m.handle(
            Event::RenewAck {
                sent_at: clock.at(20),
                ack: refused("LEASE_NOT_HOLDER"),
            },
            clock.at(21),
        );
        assert_eq!(
            out[0],
            Action::StopHub {
                reason: StandDownReason::Refused
            }
        );
        assert!(!m.is_holder_now(clock.at(21)));
        assert_eq!(
            m.handle(Event::Tick, clock.at(40)),
            vec![Action::SendAcquire { force: false }]
        );
    }

    #[test]
    fn checkpoint_mismatch_retries_at_renew_cadence_only() {
        let clock = Clock::new();
        let mut m = standby(&clock);
        m.handle(
            Event::AcquireAck {
                sent_at: clock.at(0),
                ack: refused(LEASE_CHECKPOINT_MISMATCH),
            },
            clock.at(0),
        );
        assert!(matches!(m.role(), Role::Standby { .. }));
        for t in 1..20 {
            assert!(m.handle(Event::Tick, clock.at(t)).is_empty(), "t={t}");
        }
        assert_eq!(
            m.handle(Event::Tick, clock.at(20)),
            vec![Action::SendAcquire { force: false }]
        );
    }

    #[test]
    fn capability_seen_then_missing_is_unleased_not_holder() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::SessionClosed, clock.at(5));
        let out = m.handle(Event::SessionOpened { lease_capable: false }, clock.at(6));
        assert_eq!(
            out,
            vec![Action::StopHub {
                reason: StandDownReason::RelayLeaseUnsupported
            }]
        );
        assert_eq!(
            m.role(),
            &Role::Unleased {
                reason: RELAY_LEASE_UNSUPPORTED.into()
            }
        );
        assert_eq!(m.status_hint(), Some(RELAY_LEASE_UNSUPPORTED_HINT));
        assert!(!m.is_holder_now(clock.at(6)));
        assert!(m.handle(Event::Tick, clock.at(100)).is_empty());
        assert!(m.snapshot().lease_capable);
    }

    #[test]
    fn latched_boot_with_incapable_relay_is_unleased() {
        let clock = Clock::new();
        let (mut m, out) = RoleMachine::new(
            RoleConfig::default(),
            "mach-a",
            true,
            Some(latched(LastRole::Holder, 5, Some(1))),
            clock.at(0),
        );
        assert!(out.is_empty());
        let opened = m.handle(Event::SessionOpened { lease_capable: false }, clock.at(0));
        assert!(opened.is_empty());
        assert!(matches!(m.role(), Role::Unleased { .. }));
    }

    #[test]
    fn unavailable_lease_unsupported_ack_is_unleased() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::Tick, clock.at(20));
        let out = m.handle(
            Event::RenewAck {
                sent_at: clock.at(20),
                ack: LeaseAck::Unavailable {
                    reason_code: lease::LEASE_UNSUPPORTED.into(),
                },
            },
            clock.at(21),
        );
        assert!(has(
            &out,
            &Action::StopHub {
                reason: StandDownReason::RelayLeaseUnsupported
            }
        ));
        assert!(matches!(m.role(), Role::Unleased { .. }));
    }

    #[test]
    fn capability_reset_clears_the_latch() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::SessionClosed, clock.at(1));
        m.handle(Event::SessionOpened { lease_capable: false }, clock.at(2));
        let out = m.handle(Event::CapabilityReset, clock.at(3));
        assert!(matches!(&out[0], Action::Persist(p) if !p.lease_capable));
        assert!(has(&out, &Action::StartHub));
        assert_eq!(m.role(), &Role::Holder { fence: 0 });
        assert!(!m.leased());
    }

    #[test]
    fn provisional_when_last_holder_and_single_machine() {
        let clock = Clock::new();
        let (mut m, out) = RoleMachine::new(
            RoleConfig::default(),
            "mach-a",
            true,
            Some(latched(LastRole::Holder, 5, Some(1))),
            clock.at(0),
        );
        assert!(out.is_empty());
        assert_eq!(
            m.handle(Event::RelayUnreachableAtBoot, clock.at(1)),
            vec![Action::StartHub]
        );
        assert_eq!(m.role(), &Role::Provisional);
        assert!(m.may_act_now(clock.skewed(1, 99_999)));
        assert!(!m.is_holder_now(clock.at(1)));
        // A later capable session renews the persisted fence first.
        assert_eq!(
            m.handle(Event::SessionOpened { lease_capable: true }, clock.at(50)),
            vec![Action::SendRenew { fence: 5 }]
        );
    }

    #[test]
    fn standby_when_not_last_holder_or_multi_machine() {
        let clock = Clock::new();
        for persisted in [
            latched(LastRole::Standby, 5, Some(1)),
            latched(LastRole::Holder, 5, Some(2)),
            latched(LastRole::Holder, 5, None),
        ] {
            let (mut m, _) = RoleMachine::new(
                RoleConfig::default(),
                "mach-a",
                true,
                Some(persisted.clone()),
                clock.at(0),
            );
            assert!(m.handle(Event::RelayUnreachableAtBoot, clock.at(1)).is_empty());
            assert_eq!(m.role(), &Role::standby_unknown(), "{persisted:?}");
        }
    }

    #[test]
    fn release_stands_down_and_pauses_auto_acquire() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        let out = m.handle(Event::ReleaseRequested, clock.at(5));
        assert_eq!(out[0], Action::SendRelease { fence: 5 });
        assert_eq!(
            out[1],
            Action::StopHub {
                reason: StandDownReason::Released
            }
        );
        assert!(m.handle(Event::Tick, clock.at(100)).is_empty());
        // A grant for a request sent before the release is ignored.
        let late = m.handle(
            Event::RenewAck {
                sent_at: clock.at(4),
                ack: granted(5),
            },
            clock.at(6),
        );
        assert!(late.is_empty());
        assert_eq!(
            m.handle(Event::AcquireRequested { force: true }, clock.at(101)),
            vec![Action::SendAcquire { force: true }]
        );
    }

    #[test]
    fn stale_refusal_does_not_override_newer_grant() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::Tick, clock.at(20));
        m.handle(
            Event::RenewAck {
                sent_at: clock.at(20),
                ack: granted(5),
            },
            clock.at(21),
        );
        let stale = m.handle(
            Event::AcquireAck {
                sent_at: clock.at(10),
                ack: refused("LEASE_HELD"),
            },
            clock.at(22),
        );
        assert!(stale.is_empty());
        assert_eq!(m.role(), &Role::Holder { fence: 5 });
    }

    #[test]
    fn no_requests_without_a_session() {
        let clock = Clock::new();
        let mut m = holder(&clock);
        m.handle(Event::SessionClosed, clock.at(1));
        assert!(m.handle(Event::Tick, clock.at(40)).is_empty());
    }

    #[test]
    fn wire_ack_mapping() {
        let ack = |outcome, fence, reason: Option<&str>| SupervisorLeaseAck {
            request_id: "r".into(),
            outcome,
            fence,
            holder_machine_id: None,
            holder_label: None,
            expires_in_secs: None,
            previous_expires_in_secs: None,
            checkpoint_id: None,
            account_machine_count: Some(1),
            reason_code: reason.map(str::to_string),
        };
        assert_eq!(
            LeaseAck::from_wire(&ack(SupervisorLeaseOutcome::Renewed, Some(4), None)),
            LeaseAck::Granted {
                fence: 4,
                account_machine_count: Some(1)
            }
        );
        assert!(matches!(
            LeaseAck::from_wire(&ack(SupervisorLeaseOutcome::Granted, None, None)),
            LeaseAck::Unavailable { .. }
        ));
        assert!(matches!(
            LeaseAck::from_wire(&ack(
                SupervisorLeaseOutcome::Refused,
                None,
                Some(LEASE_CHECKPOINT_MISMATCH)
            )),
            LeaseAck::Refused { reason_code, .. } if reason_code == LEASE_CHECKPOINT_MISMATCH
        ));
        assert_eq!(
            LeaseAck::from_wire(&ack(SupervisorLeaseOutcome::Unavailable, None, None)),
            LeaseAck::Unavailable {
                reason_code: lease::LEASE_UNSUPPORTED.into()
            }
        );
    }

    #[test]
    fn persisted_json_round_trips_with_defaults() {
        let parsed: PersistedLease =
            serde_json::from_str(r#"{"last_role":"holder","last_fence":9}"#).unwrap();
        assert_eq!(parsed.last_role, LastRole::Holder);
        assert!(!parsed.lease_capable);
        let lease = latched(LastRole::Standby, 3, Some(2));
        let text = serde_json::to_string(&lease).unwrap();
        assert_eq!(serde_json::from_str::<PersistedLease>(&text).unwrap(), lease);
    }
}
