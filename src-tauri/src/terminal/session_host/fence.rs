//! Host fencing state machine (design rev3 sections 2 and 5).
//!
//! Pure: OS process handles sit behind [ProcessHandle], time is an injected [HostTime], and the
//! predecessor-exit observation is an injected fact. The host wraps one [HostState] in one mutex
//! and applies every ownership or lifetime frame through it, one at a time.

use std::collections::VecDeque;
use std::io;
use std::time::{Duration, Instant};

use thiserror::Error;

use super::protocol::{
    ct_eq_32, AuthorizeTransferFrame, Epoch, HelloFrame, RejectCode, Role, Secret32,
    HOST_PROTOCOL_VERSION,
};

pub const GRANT_TTL: Duration = Duration::from_secs(90);
pub const LINGER_TIMEOUT: Duration = Duration::from_secs(600);
const RESOLVED_GRANT_HISTORY: usize = 8;

/// Monotonic time since a clock origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostTime(Duration);

impl HostTime {
    pub const fn from_duration(since_origin: Duration) -> Self {
        Self(since_origin)
    }

    pub const fn from_millis(millis: u64) -> Self {
        Self(Duration::from_millis(millis))
    }

    pub fn saturating_add(self, delta: Duration) -> Self {
        Self(self.0.saturating_add(delta))
    }

    pub fn saturating_since(self, earlier: HostTime) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

pub trait Clock {
    fn now(&self) -> HostTime;
}

#[derive(Debug, Clone, Copy)]
pub struct MonotonicClock {
    origin: Instant,
}

impl MonotonicClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for MonotonicClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MonotonicClock {
    fn now(&self) -> HostTime {
        HostTime(self.origin.elapsed())
    }
}

/// Host-assigned id of one authenticated pipe connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub creation_time: u64,
}

/// A process handle the host holds (Windows: SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION).
pub trait ProcessHandle: Sized {
    fn duplicate(&self) -> io::Result<Self>;
}

/// A process the host opened itself, with the creation time it read through that handle.
#[derive(Debug)]
pub struct OpenedProcess<H> {
    pub creation_time: u64,
    pub handle: H,
}

/// What the host observed about the peer of a connection, never what the peer claims.
#[derive(Debug)]
pub struct PeerFacts<H> {
    /// GetNamedPipeClientProcessId; None when the query failed.
    pub client_pid: Option<u32>,
    /// None when OpenProcess or the creation-time query failed.
    pub process: Option<OpenedProcess<H>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Running,
    Closing,
    Exited,
    /// Release was admitted: the host is deleting its record and exiting. Terminal; no Claim,
    /// transfer, activate or second Release is admitted from here.
    Released,
}

#[derive(Debug)]
struct ActiveConn<H> {
    conn: ConnId,
    handle: Option<H>,
}

#[derive(Debug, Clone, Copy)]
struct StandbyConn {
    conn: ConnId,
    identity: ProcessIdentity,
}

#[derive(Debug)]
struct TransferGrant<H> {
    to_epoch: Epoch,
    nonce_hash: [u8; 32],
    expires_at: HostTime,
    predecessor: H,
    successor: ProcessIdentity,
    standby: Option<StandbyConn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantEnd {
    Consumed,
    Expired,
    Revoked,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedGrant {
    to_epoch: Epoch,
    nonce_hash: [u8; 32],
    end: GrantEnd,
}

/// Side effects the host must carry out after a transition (drained with [HostState::drain_effects]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEffect {
    DisconnectStandby {
        conn: ConnId,
        to_epoch: Epoch,
        end: GrantEnd,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admission {
    pub role: Role,
    pub current_epoch: Epoch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantIssued {
    pub from_epoch: Epoch,
    pub to_epoch: Epoch,
    pub expires_in_ms: u32,
}

/// Returned by [HostState::activate_precheck]. The host waits on the predecessor handle outside
/// the lock until it is signaled or the expiry passes, then calls [HostState::activate_commit].
#[derive(Debug)]
pub struct ActivateTicket<H> {
    pub to_epoch: Epoch,
    pub expires_at: HostTime,
    pub predecessor: H,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FenceError {
    #[error("token mismatch")]
    BadToken,
    #[error("hello controllerPid {claimed} does not match pipe client pid {observed:?}")]
    ControllerPidMismatch { claimed: u32, observed: Option<u32> },
    #[error("host protocol {0} is not supported")]
    ProtocolUnsupported(u32),
    #[error("an active controller is connected")]
    ActivePresent,
    #[error("a transfer grant is pending")]
    TransferInProgress,
    #[error("the session is closing or has exited")]
    Closing,
    #[error("epoch {got} is stale (current {current})")]
    StaleEpoch { got: Epoch, current: Epoch },
    #[error("epoch {got} is burned (refused below {burned_below})")]
    EpochBurned { got: Epoch, burned_below: Epoch },
    #[error("no matching transfer grant")]
    NoGrant,
    #[error("the grant already has a standby")]
    StandbyTaken,
    #[error("the connecting process is not the named successor")]
    SuccessorMismatch,
    #[error("frame is not allowed from the standby connection")]
    NotStandby,
    #[error("frame did not arrive on the active connection")]
    NotActive,
    #[error("the transfer grant was already consumed")]
    GrantConsumed,
    #[error("the transfer grant expired")]
    GrantExpired,
    #[error("the predecessor controller has not exited")]
    PredecessorAlive,
    #[error("the active controller has no verified process handle")]
    TransferUnverifiable,
    #[error("duplicating the predecessor handle failed: {0}")]
    HandleDuplicateFailed(String),
    #[error("the session has not exited")]
    NotExited,
}

impl FenceError {
    /// Wire code for the Rejected reply. The design's code list is frozen, so a Claim pid
    /// mismatch is reported as an authentication failure and Release before exit as Busy.
    pub fn reject_code(&self) -> RejectCode {
        match self {
            Self::BadToken | Self::ControllerPidMismatch { .. } => RejectCode::BadToken,
            Self::ProtocolUnsupported(_) => RejectCode::ProtocolUnsupported,
            Self::ActivePresent => RejectCode::ActivePresent,
            Self::TransferInProgress => RejectCode::TransferInProgress,
            Self::Closing => RejectCode::Closing,
            Self::StaleEpoch { .. } => RejectCode::StaleEpoch,
            Self::EpochBurned { .. } => RejectCode::EpochBurned,
            Self::NoGrant => RejectCode::NoGrant,
            Self::StandbyTaken | Self::NotExited => RejectCode::Busy,
            Self::SuccessorMismatch => RejectCode::SuccessorMismatch,
            Self::NotStandby => RejectCode::NotStandby,
            Self::NotActive => RejectCode::NotActive,
            Self::GrantConsumed => RejectCode::GrantConsumed,
            Self::GrantExpired => RejectCode::GrantExpired,
            Self::PredecessorAlive => RejectCode::PredecessorAlive,
            Self::TransferUnverifiable | Self::HandleDuplicateFailed(_) => {
                RejectCode::TransferUnverifiable
            }
        }
    }
}

#[derive(Debug)]
pub struct HostState<H> {
    token: Secret32,
    current: Epoch,
    active: Option<ActiveConn<H>>,
    grant: Option<TransferGrant<H>>,
    burned_below: Epoch,
    phase: Phase,
    exit_code: Option<i32>,
    resolved: VecDeque<ResolvedGrant>,
    effects: Vec<HostEffect>,
    idle_since: Option<HostTime>,
    exited_at: Option<HostTime>,
}

impl<H: ProcessHandle> HostState<H> {
    pub fn new(token: Secret32, now: HostTime) -> Self {
        Self {
            token,
            current: Epoch::ZERO,
            active: None,
            grant: None,
            burned_below: Epoch::ZERO,
            phase: Phase::Running,
            exit_code: None,
            resolved: VecDeque::new(),
            effects: Vec::new(),
            idle_since: Some(now),
            exited_at: None,
        }
    }

    pub fn current_epoch(&self) -> Epoch {
        self.current
    }

    pub fn burned_below(&self) -> Epoch {
        self.burned_below
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    pub fn active_conn(&self) -> Option<ConnId> {
        self.active.as_ref().map(|a| a.conn)
    }

    pub fn standby_conn(&self) -> Option<ConnId> {
        self.grant.as_ref().and_then(|g| g.standby).map(|s| s.conn)
    }

    pub fn pending_grant_epoch(&self) -> Option<Epoch> {
        self.grant.as_ref().map(|g| g.to_epoch)
    }

    pub fn drain_effects(&mut self) -> Vec<HostEffect> {
        std::mem::take(&mut self.effects)
    }

    /// Applies grant expiry when due; every other transition calls this first.
    pub fn expire_if_due(&mut self, now: HostTime) -> bool {
        let due = self.grant.as_ref().is_some_and(|g| g.expires_at <= now);
        if due {
            self.end_grant(GrantEnd::Expired, now);
        }
        due
    }

    pub fn admit_hello(
        &mut self,
        conn: ConnId,
        hello: &HelloFrame,
        peer: PeerFacts<H>,
        now: HostTime,
    ) -> Result<Admission, FenceError> {
        if !hello.token.ct_eq(&self.token) {
            return Err(FenceError::BadToken);
        }
        if hello.host_protocol != HOST_PROTOCOL_VERSION {
            return Err(FenceError::ProtocolUnsupported(hello.host_protocol));
        }
        self.expire_if_due(now);
        match hello.role {
            Role::Claim => self.admit_claim(conn, hello, peer),
            Role::Standby => self.admit_standby(conn, hello, &peer),
        }
    }

    fn admit_claim(
        &mut self,
        conn: ConnId,
        hello: &HelloFrame,
        peer: PeerFacts<H>,
    ) -> Result<Admission, FenceError> {
        if peer.client_pid != Some(hello.controller_pid) {
            return Err(FenceError::ControllerPidMismatch {
                claimed: hello.controller_pid,
                observed: peer.client_pid,
            });
        }
        if self.active.is_some() {
            return Err(FenceError::ActivePresent);
        }
        if self.grant.is_some() {
            return Err(FenceError::TransferInProgress);
        }
        if matches!(self.phase, Phase::Closing | Phase::Released) {
            return Err(FenceError::Closing);
        }
        self.check_new_epoch(hello.controller_epoch)?;
        self.current = hello.controller_epoch;
        self.active = Some(ActiveConn {
            conn,
            handle: peer.process.map(|opened| opened.handle),
        });
        self.idle_since = None;
        Ok(Admission {
            role: Role::Claim,
            current_epoch: self.current,
        })
    }

    fn admit_standby(
        &mut self,
        conn: ConnId,
        hello: &HelloFrame,
        peer: &PeerFacts<H>,
    ) -> Result<Admission, FenceError> {
        let Some(grant) = self.grant.as_mut() else {
            return Err(FenceError::NoGrant);
        };
        let nonce_ok = hello
            .grant_nonce
            .as_ref()
            .is_some_and(|nonce| ct_eq_32(&nonce.sha256(), &grant.nonce_hash));
        if grant.to_epoch != hello.controller_epoch || !nonce_ok {
            return Err(FenceError::NoGrant);
        }
        if grant.standby.is_some() {
            return Err(FenceError::StandbyTaken);
        }
        let observed = match (peer.client_pid, peer.process.as_ref()) {
            (Some(pid), Some(opened)) => ProcessIdentity {
                pid,
                creation_time: opened.creation_time,
            },
            _ => return Err(FenceError::SuccessorMismatch),
        };
        if observed != grant.successor || hello.controller_pid != observed.pid {
            return Err(FenceError::SuccessorMismatch);
        }
        grant.standby = Some(StandbyConn {
            conn,
            identity: observed,
        });
        Ok(Admission {
            role: Role::Standby,
            current_epoch: self.current,
        })
    }

    pub fn authorize_transfer(
        &mut self,
        conn: ConnId,
        frame: &AuthorizeTransferFrame,
        now: HostTime,
    ) -> Result<GrantIssued, FenceError> {
        self.expire_if_due(now);
        let active = self
            .active
            .as_ref()
            .filter(|a| a.conn == conn)
            .ok_or(FenceError::NotActive)?;
        if frame.controller_epoch != self.current {
            return Err(FenceError::StaleEpoch {
                got: frame.controller_epoch,
                current: self.current,
            });
        }
        self.check_new_epoch(frame.to_epoch)?;
        if self.phase != Phase::Running {
            return Err(FenceError::Closing);
        }
        if self.grant.is_some() {
            return Err(FenceError::TransferInProgress);
        }
        let handle = active
            .handle
            .as_ref()
            .ok_or(FenceError::TransferUnverifiable)?;
        let predecessor = handle
            .duplicate()
            .map_err(|e| FenceError::HandleDuplicateFailed(e.to_string()))?;
        self.grant = Some(TransferGrant {
            to_epoch: frame.to_epoch,
            nonce_hash: frame.grant_nonce.sha256(),
            expires_at: now.saturating_add(GRANT_TTL),
            predecessor,
            successor: ProcessIdentity {
                pid: frame.successor_pid,
                creation_time: frame.successor_creation_time,
            },
            standby: None,
        });
        let expires_in_ms = u32::try_from(GRANT_TTL.as_millis()).unwrap_or(u32::MAX);
        Ok(GrantIssued {
            from_epoch: self.current,
            to_epoch: frame.to_epoch,
            expires_in_ms,
        })
    }

    pub fn revoke_transfer(
        &mut self,
        conn: ConnId,
        controller_epoch: Epoch,
        to_epoch: Epoch,
        now: HostTime,
    ) -> Result<Epoch, FenceError> {
        self.expire_if_due(now);
        if self.active_conn() != Some(conn) {
            return Err(FenceError::NotActive);
        }
        if controller_epoch != self.current {
            return Err(FenceError::StaleEpoch {
                got: controller_epoch,
                current: self.current,
            });
        }
        if self.pending_grant_epoch() != Some(to_epoch) {
            return Err(FenceError::NoGrant);
        }
        self.end_grant(GrantEnd::Revoked, now);
        Ok(to_epoch)
    }

    /// Step 1 of Activate, under the lock. controller_epoch is the Activate frame's epoch and
    /// must equal the grant's toEpoch.
    pub fn activate_precheck(
        &mut self,
        conn: ConnId,
        controller_epoch: Epoch,
        grant_nonce: &Secret32,
        now: HostTime,
    ) -> Result<ActivateTicket<H>, FenceError> {
        self.expire_if_due(now);
        let grant = self.activate_checks(conn, controller_epoch, grant_nonce)?;
        if self.active.is_some() {
            return Err(FenceError::PredecessorAlive);
        }
        let predecessor = grant
            .predecessor
            .duplicate()
            .map_err(|e| FenceError::HandleDuplicateFailed(e.to_string()))?;
        Ok(ActivateTicket {
            to_epoch: grant.to_epoch,
            expires_at: grant.expires_at,
            predecessor,
        })
    }

    /// Step 2 of Activate, back under the lock after the wait. predecessor_exited is the host's
    /// own observation that the predecessor handle is signaled. successor_process is the handle
    /// the host opens for the standby now; a creation-time mismatch discards it, which leaves the
    /// new active unable to authorize a later transfer.
    pub fn activate_commit(
        &mut self,
        conn: ConnId,
        controller_epoch: Epoch,
        grant_nonce: &Secret32,
        predecessor_exited: bool,
        successor_process: Option<OpenedProcess<H>>,
        now: HostTime,
    ) -> Result<Epoch, FenceError> {
        self.expire_if_due(now);
        let grant = self.activate_checks(conn, controller_epoch, grant_nonce)?;
        let standby = grant.standby.ok_or(FenceError::NotStandby)?;
        if self.active.is_some() || !predecessor_exited {
            return Err(FenceError::PredecessorAlive);
        }
        let Some(grant) = self.grant.take() else {
            return Err(FenceError::NoGrant);
        };
        self.remember(&grant, GrantEnd::Consumed);
        self.current = grant.to_epoch;
        self.burned_below = self.burned_below.max(grant.to_epoch);
        let handle = successor_process
            .filter(|opened| opened.creation_time == standby.identity.creation_time)
            .map(|opened| opened.handle);
        self.active = Some(ActiveConn { conn, handle });
        self.idle_since = None;
        Ok(grant.to_epoch)
    }

    /// Input, Resize and SnapshotRequest gate: active connection AND current epoch.
    pub fn check_controller(
        &mut self,
        conn: ConnId,
        epoch: Epoch,
        now: HostTime,
    ) -> Result<(), FenceError> {
        self.expire_if_due(now);
        self.controller_checks(conn, epoch)
    }

    /// Close step 1. On success the phase is Closing and the host may drop the input writer.
    pub fn begin_close(
        &mut self,
        conn: ConnId,
        epoch: Epoch,
        now: HostTime,
    ) -> Result<(), FenceError> {
        self.expire_if_due(now);
        self.controller_checks(conn, epoch)?;
        if self.grant.is_some() {
            return Err(FenceError::TransferInProgress);
        }
        if self.phase != Phase::Running {
            return Err(FenceError::Closing);
        }
        self.phase = Phase::Closing;
        Ok(())
    }

    /// Release after Exited was consumed. On success the phase is Released, so nothing is
    /// admitted while the host deletes its record and exits.
    pub fn release(&mut self, conn: ConnId, epoch: Epoch, now: HostTime) -> Result<(), FenceError> {
        self.expire_if_due(now);
        self.controller_checks(conn, epoch)?;
        if self.grant.is_some() {
            return Err(FenceError::TransferInProgress);
        }
        match self.phase {
            Phase::Exited => {}
            Phase::Released => return Err(FenceError::Closing),
            Phase::Running | Phase::Closing => return Err(FenceError::NotExited),
        }
        self.phase = Phase::Released;
        Ok(())
    }

    /// Natural exit or the end of Close. Any pending grant stays valid. Returns false if the
    /// session had already exited.
    pub fn mark_exited(&mut self, code: Option<i32>, now: HostTime) -> bool {
        if matches!(self.phase, Phase::Exited | Phase::Released) {
            return false;
        }
        self.phase = Phase::Exited;
        self.exit_code = code;
        self.exited_at = Some(now);
        true
    }

    /// A disconnect never kills the shell and never clears an unexpired grant.
    pub fn on_disconnect(&mut self, conn: ConnId, now: HostTime) {
        if self.active_conn() == Some(conn) {
            self.active = None;
            self.idle_since = Some(now);
        }
        if let Some(grant) = self.grant.as_mut() {
            if grant.standby.is_some_and(|s| s.conn == conn) {
                grant.standby = None;
            }
        }
    }

    /// True only after the shell exited (a Running shell survives any controller absence) and the
    /// host has since had no active controller and no grant for [LINGER_TIMEOUT], measured from
    /// the later of the last disconnect or grant end and the exit.
    pub fn linger_due(&mut self, now: HostTime) -> bool {
        self.expire_if_due(now);
        if self.phase != Phase::Exited || self.active.is_some() || self.grant.is_some() {
            return false;
        }
        match (self.idle_since, self.exited_at) {
            (Some(idle), Some(exited)) => now.saturating_since(idle.max(exited)) >= LINGER_TIMEOUT,
            _ => false,
        }
    }

    fn controller_checks(&self, conn: ConnId, epoch: Epoch) -> Result<(), FenceError> {
        if self.standby_conn() == Some(conn) {
            return Err(FenceError::NotStandby);
        }
        if self.active_conn() != Some(conn) {
            return Err(FenceError::NotActive);
        }
        if epoch != self.current {
            return Err(FenceError::StaleEpoch {
                got: epoch,
                current: self.current,
            });
        }
        Ok(())
    }

    fn check_new_epoch(&self, epoch: Epoch) -> Result<(), FenceError> {
        if epoch < self.burned_below {
            return Err(FenceError::EpochBurned {
                got: epoch,
                burned_below: self.burned_below,
            });
        }
        if epoch <= self.current {
            return Err(FenceError::StaleEpoch {
                got: epoch,
                current: self.current,
            });
        }
        Ok(())
    }

    fn activate_checks(
        &self,
        conn: ConnId,
        controller_epoch: Epoch,
        grant_nonce: &Secret32,
    ) -> Result<&TransferGrant<H>, FenceError> {
        let hash = grant_nonce.sha256();
        let grant = match self.grant.as_ref() {
            Some(grant) if ct_eq_32(&hash, &grant.nonce_hash) => grant,
            _ => return Err(self.resolved_error(&hash)),
        };
        if grant.standby.map(|s| s.conn) != Some(conn) {
            return Err(FenceError::NotStandby);
        }
        if controller_epoch != grant.to_epoch {
            return Err(FenceError::NoGrant);
        }
        if grant.to_epoch < self.burned_below {
            return Err(FenceError::EpochBurned {
                got: grant.to_epoch,
                burned_below: self.burned_below,
            });
        }
        if matches!(self.phase, Phase::Closing | Phase::Released) {
            return Err(FenceError::Closing);
        }
        Ok(grant)
    }

    fn resolved_error(&self, hash: &[u8; 32]) -> FenceError {
        match self.resolved.iter().find(|r| ct_eq_32(&r.nonce_hash, hash)) {
            Some(ResolvedGrant {
                end: GrantEnd::Consumed,
                ..
            }) => FenceError::GrantConsumed,
            Some(ResolvedGrant {
                end: GrantEnd::Expired,
                ..
            }) => FenceError::GrantExpired,
            Some(ResolvedGrant {
                end: GrantEnd::Revoked,
                to_epoch,
                ..
            }) => FenceError::EpochBurned {
                got: *to_epoch,
                burned_below: self.burned_below,
            },
            None => FenceError::NoGrant,
        }
    }

    /// Revoke and expiry share one effect: drop the grant, disconnect its standby, burn <= toEpoch.
    fn end_grant(&mut self, end: GrantEnd, now: HostTime) {
        let Some(grant) = self.grant.take() else {
            return;
        };
        self.burned_below = self.burned_below.max(grant.to_epoch.saturating_next());
        if let Some(standby) = grant.standby {
            self.effects.push(HostEffect::DisconnectStandby {
                conn: standby.conn,
                to_epoch: grant.to_epoch,
                end,
            });
        }
        self.remember(&grant, end);
        if self.active.is_none() {
            self.idle_since = Some(now);
        }
    }

    fn remember(&mut self, grant: &TransferGrant<H>, end: GrantEnd) {
        if self.resolved.len() == RESOLVED_GRANT_HISTORY {
            self.resolved.pop_front();
        }
        self.resolved.push_back(ResolvedGrant {
            to_epoch: grant.to_epoch,
            nonce_hash: grant.nonce_hash,
            end,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    const PRED_PID: u32 = 100;
    const PRED_CT: u64 = 1_000;
    const SUCC_PID: u32 = 200;
    const SUCC_CT: u64 = 2_000;
    const P: ConnId = ConnId(1);
    const S: ConnId = ConnId(2);
    const OTHER: ConnId = ConnId(3);

    #[derive(Debug, PartialEq, Eq)]
    struct FakeHandle {
        pid: u32,
        duplicate_fails: bool,
    }

    impl ProcessHandle for FakeHandle {
        fn duplicate(&self) -> io::Result<Self> {
            if self.duplicate_fails {
                return Err(io::Error::other("duplicate denied"));
            }
            Ok(FakeHandle {
                pid: self.pid,
                duplicate_fails: false,
            })
        }
    }

    /// Explicit fake clock: time moves only when a test advances it.
    struct FakeClock(Cell<HostTime>);

    impl FakeClock {
        fn new() -> Self {
            Self(Cell::new(HostTime::from_millis(0)))
        }

        fn advance(&self, delta: Duration) {
            self.0.set(self.0.get().saturating_add(delta));
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> HostTime {
            self.0.get()
        }
    }

    fn token() -> Secret32 {
        Secret32::from_bytes([7; 32])
    }

    fn nonce(byte: u8) -> Secret32 {
        Secret32::from_bytes([byte; 32])
    }

    fn hello(role: Role, epoch: u64, pid: u32, grant_nonce: Option<Secret32>) -> HelloFrame {
        HelloFrame {
            token: token(),
            host_protocol: HOST_PROTOCOL_VERSION,
            controller_epoch: Epoch(epoch),
            controller_pid: pid,
            role,
            grant_nonce,
        }
    }

    fn opened(pid: u32, creation_time: u64) -> Option<OpenedProcess<FakeHandle>> {
        Some(OpenedProcess {
            creation_time,
            handle: FakeHandle {
                pid,
                duplicate_fails: false,
            },
        })
    }

    fn peer(pid: u32, creation_time: u64) -> PeerFacts<FakeHandle> {
        PeerFacts {
            client_pid: Some(pid),
            process: opened(pid, creation_time),
        }
    }

    fn authorize_frame(from: u64, to: u64, grant: Secret32) -> AuthorizeTransferFrame {
        AuthorizeTransferFrame {
            controller_epoch: Epoch(from),
            to_epoch: Epoch(to),
            grant_nonce: grant,
            successor_pid: SUCC_PID,
            successor_creation_time: SUCC_CT,
        }
    }

    /// Host claimed by P at epoch 1.
    fn claimed(clock: &FakeClock) -> HostState<FakeHandle> {
        let mut state = HostState::new(token(), clock.now());
        state
            .admit_hello(
                P,
                &hello(Role::Claim, 1, PRED_PID, None),
                peer(PRED_PID, PRED_CT),
                clock.now(),
            )
            .unwrap();
        state
    }

    /// Host claimed by P at epoch 1 with a grant to epoch 2 and S bound as standby.
    fn granted(clock: &FakeClock) -> HostState<FakeHandle> {
        let mut state = claimed(clock);
        state
            .authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now())
            .unwrap();
        let standby = hello(Role::Standby, 2, SUCC_PID, Some(nonce(9)));
        state
            .admit_hello(S, &standby, peer(SUCC_PID, SUCC_CT), clock.now())
            .unwrap();
        state
    }

    fn code<T: std::fmt::Debug>(result: Result<T, FenceError>) -> RejectCode {
        result.unwrap_err().reject_code()
    }

    #[test]
    fn claim_while_active_present_is_rejected_at_any_higher_epoch() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        for epoch in [2, 50, u64::MAX] {
            let result = state.admit_hello(
                OTHER,
                &hello(Role::Claim, epoch, 300, None),
                peer(300, 3),
                clock.now(),
            );
            assert_eq!(result, Err(FenceError::ActivePresent));
        }
        assert_eq!(state.current_epoch(), Epoch(1));
        assert_eq!(state.active_conn(), Some(P));
    }

    #[test]
    fn claim_requires_token_protocol_and_matching_pipe_client_pid() {
        let clock = FakeClock::new();
        let mut state: HostState<FakeHandle> = HostState::new(token(), clock.now());
        let mut bad_token = hello(Role::Claim, 1, PRED_PID, None);
        bad_token.token = nonce(1);
        assert_eq!(
            state.admit_hello(P, &bad_token, peer(PRED_PID, PRED_CT), clock.now()),
            Err(FenceError::BadToken)
        );
        let mut bad_protocol = hello(Role::Claim, 1, PRED_PID, None);
        bad_protocol.host_protocol = 2;
        let result = state.admit_hello(P, &bad_protocol, peer(PRED_PID, PRED_CT), clock.now());
        assert_eq!(result, Err(FenceError::ProtocolUnsupported(2)));
        let spoofed = state.admit_hello(
            P,
            &hello(Role::Claim, 1, PRED_PID, None),
            peer(999, PRED_CT),
            clock.now(),
        );
        assert_eq!(code(spoofed), RejectCode::BadToken);
        assert_eq!(state.active_conn(), None);
    }

    #[test]
    fn claim_after_disconnect_needs_a_strictly_higher_epoch() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        state.on_disconnect(P, clock.now());
        let same = state.admit_hello(
            OTHER,
            &hello(Role::Claim, 1, 300, None),
            peer(300, 3),
            clock.now(),
        );
        assert_eq!(code(same), RejectCode::StaleEpoch);
        let admitted = state.admit_hello(
            OTHER,
            &hello(Role::Claim, 2, 300, None),
            peer(300, 3),
            clock.now(),
        );
        assert_eq!(
            admitted,
            Ok(Admission {
                role: Role::Claim,
                current_epoch: Epoch(2)
            })
        );
    }

    #[test]
    fn claim_is_refused_while_a_grant_is_pending_even_without_an_active() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(P, clock.now());
        let result = state.admit_hello(
            OTHER,
            &hello(Role::Claim, 9, 300, None),
            peer(300, 3),
            clock.now(),
        );
        assert_eq!(result, Err(FenceError::TransferInProgress));
    }

    #[test]
    fn standby_without_or_with_wrong_nonce_or_epoch_gets_no_grant() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        let no_grant_yet = state.admit_hello(
            S,
            &hello(Role::Standby, 2, SUCC_PID, Some(nonce(9))),
            peer(SUCC_PID, SUCC_CT),
            clock.now(),
        );
        assert_eq!(no_grant_yet, Err(FenceError::NoGrant));
        state
            .authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now())
            .unwrap();
        for (epoch, grant) in [(2, None), (2, Some(nonce(8))), (3, Some(nonce(9)))] {
            let result = state.admit_hello(
                S,
                &hello(Role::Standby, epoch, SUCC_PID, grant),
                peer(SUCC_PID, SUCC_CT),
                clock.now(),
            );
            assert_eq!(result, Err(FenceError::NoGrant));
        }
        assert_eq!(state.standby_conn(), None);
    }

    #[test]
    fn standby_from_wrong_process_identity_gets_successor_mismatch() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        state
            .authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now())
            .unwrap();
        let standby = hello(Role::Standby, 2, SUCC_PID, Some(nonce(9)));
        let reused_pid = state.admit_hello(S, &standby, peer(SUCC_PID, SUCC_CT + 1), clock.now());
        assert_eq!(reused_pid, Err(FenceError::SuccessorMismatch));
        let other_pid = state.admit_hello(S, &standby, peer(SUCC_PID + 1, SUCC_CT), clock.now());
        assert_eq!(other_pid, Err(FenceError::SuccessorMismatch));
        let query_failed = PeerFacts {
            client_pid: Some(SUCC_PID),
            process: None,
        };
        assert_eq!(
            state.admit_hello(S, &standby, query_failed, clock.now()),
            Err(FenceError::SuccessorMismatch)
        );
        assert_eq!(state.standby_conn(), None);
    }

    #[test]
    fn second_standby_for_the_same_grant_is_busy() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        let second = hello(Role::Standby, 2, SUCC_PID, Some(nonce(9)));
        assert_eq!(
            code(state.admit_hello(OTHER, &second, peer(SUCC_PID, SUCC_CT), clock.now())),
            RejectCode::Busy
        );
        assert_eq!(state.standby_conn(), Some(S));
    }

    #[test]
    fn controller_frames_need_the_active_connection_and_current_epoch() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        assert_eq!(
            state.check_controller(S, Epoch(1), clock.now()),
            Err(FenceError::NotStandby)
        );
        assert_eq!(
            state.check_controller(OTHER, Epoch(1), clock.now()),
            Err(FenceError::NotActive)
        );
        assert_eq!(
            code(state.check_controller(P, Epoch(2), clock.now())),
            RejectCode::StaleEpoch
        );
        assert_eq!(
            state.begin_close(S, Epoch(1), clock.now()),
            Err(FenceError::NotStandby)
        );
        assert_eq!(
            state.begin_close(OTHER, Epoch(1), clock.now()),
            Err(FenceError::NotActive)
        );
        assert_eq!(
            state.release(S, Epoch(1), clock.now()),
            Err(FenceError::NotStandby)
        );
        assert_eq!(
            state.release(OTHER, Epoch(1), clock.now()),
            Err(FenceError::NotActive)
        );
        assert_eq!(state.check_controller(P, Epoch(1), clock.now()), Ok(()));
        assert_eq!(state.phase(), Phase::Running);
    }

    #[test]
    fn activate_while_predecessor_connected_or_alive_is_predecessor_alive() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        assert_eq!(
            state
                .activate_precheck(S, Epoch(2), &nonce(9), clock.now())
                .unwrap_err(),
            FenceError::PredecessorAlive
        );
        state.on_disconnect(P, clock.now());
        let ticket = state.activate_precheck(S, Epoch(2), &nonce(9), clock.now()).unwrap();
        assert_eq!(ticket.to_epoch, Epoch(2));
        assert_eq!(ticket.predecessor.pid, PRED_PID);
        assert_eq!(ticket.expires_at, clock.now().saturating_add(GRANT_TTL));
        let alive = state.activate_commit(
            S,
            Epoch(2),
            &nonce(9),
            false,
            opened(SUCC_PID, SUCC_CT),
            clock.now(),
        );
        assert_eq!(alive, Err(FenceError::PredecessorAlive));
        assert_eq!(state.pending_grant_epoch(), Some(Epoch(2)));
        assert_eq!(state.current_epoch(), Epoch(1));
    }

    #[test]
    fn activate_from_a_non_standby_connection_is_not_standby() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(P, clock.now());
        let result = state.activate_commit(
            OTHER,
            Epoch(2),
            &nonce(9),
            true,
            opened(SUCC_PID, SUCC_CT),
            clock.now(),
        );
        assert_eq!(result, Err(FenceError::NotStandby));
        assert_eq!(
            state
                .activate_precheck(S, Epoch(2), &nonce(8), clock.now())
                .unwrap_err(),
            FenceError::NoGrant
        );
    }

    #[test]
    fn activate_at_expiry_is_grant_expired_without_any_timer() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(P, clock.now());
        clock.advance(GRANT_TTL);
        let result = state.activate_commit(
            S,
            Epoch(2),
            &nonce(9),
            true,
            opened(SUCC_PID, SUCC_CT),
            clock.now(),
        );
        assert_eq!(result, Err(FenceError::GrantExpired));
        assert_eq!(state.burned_below(), Epoch(3));
        assert_eq!(state.active_conn(), None);
        let expected = HostEffect::DisconnectStandby {
            conn: S,
            to_epoch: Epoch(2),
            end: GrantEnd::Expired,
        };
        assert_eq!(state.drain_effects(), vec![expected]);
    }

    #[test]
    fn activate_one_millisecond_before_expiry_succeeds() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(P, clock.now());
        clock.advance(GRANT_TTL - Duration::from_millis(1));
        let result = state.activate_commit(
            S,
            Epoch(2),
            &nonce(9),
            true,
            opened(SUCC_PID, SUCC_CT),
            clock.now(),
        );
        assert_eq!(result, Ok(Epoch(2)));
    }

    #[test]
    fn activate_consumes_the_grant_and_reuse_is_grant_consumed() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(P, clock.now());
        assert_eq!(
            state.activate_commit(
                S,
                Epoch(2),
                &nonce(9),
                true,
                opened(SUCC_PID, SUCC_CT),
                clock.now(),
            ),
            Ok(Epoch(2))
        );
        assert_eq!(state.current_epoch(), Epoch(2));
        assert_eq!(state.burned_below(), Epoch(2));
        assert_eq!(state.active_conn(), Some(S));
        assert_eq!(state.pending_grant_epoch(), None);
        assert_eq!(
            state
                .activate_precheck(S, Epoch(2), &nonce(9), clock.now())
                .unwrap_err(),
            FenceError::GrantConsumed
        );
        assert_eq!(
            state.activate_commit(S, Epoch(2), &nonce(9), true, None, clock.now()),
            Err(FenceError::GrantConsumed)
        );
        assert_eq!(
            state.check_controller(P, Epoch(1), clock.now()),
            Err(FenceError::NotActive)
        );
        assert_eq!(state.check_controller(S, Epoch(2), clock.now()), Ok(()));
        assert!(state.drain_effects().is_empty());
    }

    #[test]
    fn successor_handle_with_wrong_creation_time_leaves_it_unverifiable() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(P, clock.now());
        state
            .activate_commit(
                S,
                Epoch(2),
                &nonce(9),
                true,
                opened(SUCC_PID, SUCC_CT + 1),
                clock.now(),
            )
            .unwrap();
        let next = state.authorize_transfer(S, &authorize_frame(2, 3, nonce(4)), clock.now());
        assert_eq!(next, Err(FenceError::TransferUnverifiable));
    }

    #[test]
    fn revoke_burns_through_to_epoch_and_disconnects_the_standby() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        assert_eq!(
            state.revoke_transfer(P, Epoch(1), Epoch(3), clock.now()),
            Err(FenceError::NoGrant)
        );
        assert_eq!(
            state.revoke_transfer(S, Epoch(1), Epoch(2), clock.now()),
            Err(FenceError::NotActive)
        );
        assert_eq!(
            code(state.revoke_transfer(P, Epoch(0), Epoch(2), clock.now())),
            RejectCode::StaleEpoch
        );
        assert_eq!(
            state.revoke_transfer(P, Epoch(1), Epoch(2), clock.now()),
            Ok(Epoch(2))
        );
        assert_eq!(state.burned_below(), Epoch(3));
        let expected = HostEffect::DisconnectStandby {
            conn: S,
            to_epoch: Epoch(2),
            end: GrantEnd::Revoked,
        };
        assert_eq!(state.drain_effects(), vec![expected]);
        for burned in [2, 1] {
            let result =
                state.authorize_transfer(P, &authorize_frame(1, burned, nonce(5)), clock.now());
            assert_eq!(code(result), RejectCode::EpochBurned);
        }
        assert!(state
            .authorize_transfer(P, &authorize_frame(1, 3, nonce(5)), clock.now())
            .is_ok());
        let replay = hello(Role::Standby, 2, SUCC_PID, Some(nonce(9)));
        assert_eq!(
            state.admit_hello(S, &replay, peer(SUCC_PID, SUCC_CT), clock.now()),
            Err(FenceError::NoGrant)
        );
    }

    #[test]
    fn activate_with_a_revoked_nonce_is_epoch_burned() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state
            .revoke_transfer(P, Epoch(1), Epoch(2), clock.now())
            .unwrap();
        assert_eq!(
            code(state.activate_precheck(S, Epoch(2), &nonce(9), clock.now())),
            RejectCode::EpochBurned
        );
    }

    #[test]
    fn expiry_burns_like_revoke_and_leaves_the_next_epoch_usable() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        clock.advance(GRANT_TTL);
        assert!(state.expire_if_due(clock.now()));
        assert!(!state.expire_if_due(clock.now()));
        assert_eq!(state.burned_below(), Epoch(3));
        assert_eq!(state.pending_grant_epoch(), None);
        let burned = state.authorize_transfer(P, &authorize_frame(1, 2, nonce(5)), clock.now());
        assert_eq!(code(burned), RejectCode::EpochBurned);
        assert!(state
            .authorize_transfer(P, &authorize_frame(1, 3, nonce(5)), clock.now())
            .is_ok());
    }

    #[test]
    fn inline_expiry_runs_before_every_frame_check() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        clock.advance(GRANT_TTL + Duration::from_secs(1));
        assert_eq!(state.begin_close(P, Epoch(1), clock.now()), Ok(()));
        assert_eq!(state.burned_below(), Epoch(3));
        assert_eq!(state.phase(), Phase::Closing);
    }

    #[test]
    fn close_and_release_with_a_pending_grant_are_transfer_in_progress() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        assert_eq!(
            state.begin_close(P, Epoch(1), clock.now()),
            Err(FenceError::TransferInProgress)
        );
        assert_eq!(state.phase(), Phase::Running);
        assert!(state.mark_exited(Some(0), clock.now()));
        assert_eq!(
            state.release(P, Epoch(1), clock.now()),
            Err(FenceError::TransferInProgress)
        );
    }

    #[test]
    fn authorize_while_closing_or_after_exit_is_closing() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        state.begin_close(P, Epoch(1), clock.now()).unwrap();
        assert_eq!(
            state.authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now()),
            Err(FenceError::Closing)
        );
        assert_eq!(
            state.begin_close(P, Epoch(1), clock.now()),
            Err(FenceError::Closing)
        );
        let mut exited = claimed(&clock);
        exited.mark_exited(None, clock.now());
        assert_eq!(
            exited.authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now()),
            Err(FenceError::Closing)
        );
    }

    #[test]
    fn claim_while_closing_is_closing() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        state.begin_close(P, Epoch(1), clock.now()).unwrap();
        state.on_disconnect(P, clock.now());
        let result = state.admit_hello(
            OTHER,
            &hello(Role::Claim, 5, 300, None),
            peer(300, 3),
            clock.now(),
        );
        assert_eq!(result, Err(FenceError::Closing));
    }

    #[test]
    fn natural_exit_keeps_the_grant_so_the_successor_can_activate_and_release() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        assert!(state.mark_exited(Some(3), clock.now()));
        assert!(!state.mark_exited(Some(4), clock.now()));
        assert_eq!(state.pending_grant_epoch(), Some(Epoch(2)));
        state.on_disconnect(P, clock.now());
        assert_eq!(
            state.activate_commit(
                S,
                Epoch(2),
                &nonce(9),
                true,
                opened(SUCC_PID, SUCC_CT),
                clock.now(),
            ),
            Ok(Epoch(2))
        );
        assert_eq!(state.release(S, Epoch(2), clock.now()), Ok(()));
        assert_eq!(state.exit_code(), Some(3));
    }

    #[test]
    fn release_before_exit_is_refused() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        assert_eq!(
            state.release(P, Epoch(1), clock.now()),
            Err(FenceError::NotExited)
        );
        state.begin_close(P, Epoch(1), clock.now()).unwrap();
        assert_eq!(
            state.release(P, Epoch(1), clock.now()),
            Err(FenceError::NotExited)
        );
        state.mark_exited(Some(0), clock.now());
        assert_eq!(state.release(P, Epoch(1), clock.now()), Ok(()));
    }

    #[test]
    fn authorize_without_a_verified_handle_is_transfer_unverifiable() {
        let clock = FakeClock::new();
        let mut state: HostState<FakeHandle> = HostState::new(token(), clock.now());
        let unopened = PeerFacts {
            client_pid: Some(PRED_PID),
            process: None,
        };
        state
            .admit_hello(
                P,
                &hello(Role::Claim, 1, PRED_PID, None),
                unopened,
                clock.now(),
            )
            .unwrap();
        let result = state.authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now());
        assert_eq!(result, Err(FenceError::TransferUnverifiable));

        let mut dup_fails: HostState<FakeHandle> = HostState::new(token(), clock.now());
        let failing = PeerFacts {
            client_pid: Some(PRED_PID),
            process: Some(OpenedProcess {
                creation_time: PRED_CT,
                handle: FakeHandle {
                    pid: PRED_PID,
                    duplicate_fails: true,
                },
            }),
        };
        dup_fails
            .admit_hello(
                P,
                &hello(Role::Claim, 1, PRED_PID, None),
                failing,
                clock.now(),
            )
            .unwrap();
        let result = dup_fails.authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now());
        assert_eq!(code(result), RejectCode::TransferUnverifiable);
        assert_eq!(dup_fails.pending_grant_epoch(), None);
    }

    #[test]
    fn authorize_needs_active_connection_current_epoch_and_no_grant() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        let wrong_conn =
            state.authorize_transfer(OTHER, &authorize_frame(1, 2, nonce(9)), clock.now());
        assert_eq!(wrong_conn, Err(FenceError::NotActive));
        assert_eq!(
            code(state.authorize_transfer(P, &authorize_frame(0, 2, nonce(9)), clock.now())),
            RejectCode::StaleEpoch
        );
        assert_eq!(
            code(state.authorize_transfer(P, &authorize_frame(1, 1, nonce(9)), clock.now())),
            RejectCode::StaleEpoch
        );
        let issued = state
            .authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now())
            .unwrap();
        assert_eq!(
            issued,
            GrantIssued {
                from_epoch: Epoch(1),
                to_epoch: Epoch(2),
                expires_in_ms: 90_000
            }
        );
        let second = state.authorize_transfer(P, &authorize_frame(1, 5, nonce(3)), clock.now());
        assert_eq!(second, Err(FenceError::TransferInProgress));
    }

    #[test]
    fn disconnects_never_clear_an_unexpired_grant() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(S, clock.now());
        state.on_disconnect(P, clock.now());
        assert_eq!(state.pending_grant_epoch(), Some(Epoch(2)));
        assert_eq!(state.standby_conn(), None);
        let rebound = hello(Role::Standby, 2, SUCC_PID, Some(nonce(9)));
        assert!(state
            .admit_hello(OTHER, &rebound, peer(SUCC_PID, SUCC_CT), clock.now())
            .is_ok());
        assert_eq!(state.standby_conn(), Some(OTHER));
    }

    #[test]
    fn exited_linger_waits_for_no_active_and_no_grant_and_restarts_when_the_grant_resolves() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        assert!(state.mark_exited(Some(0), clock.now()));
        state.on_disconnect(P, clock.now());
        clock.advance(GRANT_TTL - Duration::from_millis(1));
        assert!(!state.linger_due(clock.now()), "a pending grant suppresses linger");
        clock.advance(Duration::from_millis(1));
        assert!(
            !state.linger_due(clock.now()),
            "expiry restarts the linger window"
        );
        clock.advance(LINGER_TIMEOUT - Duration::from_millis(1));
        assert!(!state.linger_due(clock.now()));
        clock.advance(Duration::from_millis(1));
        assert!(state.linger_due(clock.now()));
    }

    #[test]
    fn linger_never_fires_with_an_active_controller() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        assert!(state.mark_exited(Some(0), clock.now()));
        clock.advance(LINGER_TIMEOUT * 2);
        assert!(!state.linger_due(clock.now()));
    }

    #[test]
    fn running_shell_without_a_controller_never_lingers() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        state.on_disconnect(P, clock.now());
        clock.advance(LINGER_TIMEOUT * 3);
        assert_eq!(state.phase(), Phase::Running);
        assert!(!state.linger_due(clock.now()), "a live shell survives controller absence");
        assert!(state.mark_exited(Some(0), clock.now()));
        assert!(
            !state.linger_due(clock.now()),
            "the window starts at exit, not at the earlier disconnect"
        );
        clock.advance(LINGER_TIMEOUT - Duration::from_millis(1));
        assert!(!state.linger_due(clock.now()));
        clock.advance(Duration::from_millis(1));
        assert!(state.linger_due(clock.now()));
    }

    #[test]
    fn activate_epoch_must_equal_the_grant_to_epoch() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        state.on_disconnect(P, clock.now());
        for wrong in [Epoch(1), Epoch(3), Epoch(u64::MAX)] {
            assert_eq!(
                state
                    .activate_precheck(S, wrong, &nonce(9), clock.now())
                    .unwrap_err(),
                FenceError::NoGrant
            );
            let succ = opened(SUCC_PID, SUCC_CT);
            let commit = state.activate_commit(S, wrong, &nonce(9), true, succ, clock.now());
            assert_eq!(commit, Err(FenceError::NoGrant));
        }
        assert_eq!(state.pending_grant_epoch(), Some(Epoch(2)));
        assert_eq!(state.current_epoch(), Epoch(1));
        assert_eq!(state.active_conn(), None);
        let succ = opened(SUCC_PID, SUCC_CT);
        let commit = state.activate_commit(S, Epoch(2), &nonce(9), true, succ, clock.now());
        assert_eq!(commit, Ok(Epoch(2)));
    }

    #[test]
    fn release_is_terminal_and_blocks_claim_before_the_host_exits() {
        let clock = FakeClock::new();
        let mut state = claimed(&clock);
        assert!(state.mark_exited(Some(0), clock.now()));
        assert_eq!(state.release(P, Epoch(1), clock.now()), Ok(()));
        assert_eq!(state.phase(), Phase::Released);
        assert_eq!(
            state.release(P, Epoch(1), clock.now()),
            Err(FenceError::Closing)
        );
        assert_eq!(
            state.authorize_transfer(P, &authorize_frame(1, 2, nonce(9)), clock.now()),
            Err(FenceError::Closing)
        );
        state.on_disconnect(P, clock.now());
        let reclaim = state.admit_hello(
            OTHER,
            &hello(Role::Claim, 5, 300, None),
            peer(300, 3),
            clock.now(),
        );
        assert_eq!(code(reclaim), RejectCode::Closing);
        assert_eq!(state.active_conn(), None);
        assert_eq!(state.current_epoch(), Epoch(1));
        assert!(!state.mark_exited(Some(9), clock.now()));
        assert_eq!(state.phase(), Phase::Released);
        assert_eq!(state.exit_code(), Some(0));
    }

    #[test]
    fn release_after_activate_blocks_the_next_claim() {
        let clock = FakeClock::new();
        let mut state = granted(&clock);
        assert!(state.mark_exited(Some(1), clock.now()));
        state.on_disconnect(P, clock.now());
        let succ = opened(SUCC_PID, SUCC_CT);
        state
            .activate_commit(S, Epoch(2), &nonce(9), true, succ, clock.now())
            .unwrap();
        assert_eq!(state.release(S, Epoch(2), clock.now()), Ok(()));
        state.on_disconnect(S, clock.now());
        let reclaim = state.admit_hello(
            OTHER,
            &hello(Role::Claim, 7, 300, None),
            peer(300, 3),
            clock.now(),
        );
        assert_eq!(reclaim, Err(FenceError::Closing));
        assert_eq!(state.current_epoch(), Epoch(2));
    }
}
