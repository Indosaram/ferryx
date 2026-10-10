//! Identity-bound suspension. Call these synchronous functions via `run_blocking`.
//! Ownership is deliberately local to this daemon lifetime: a receipt is not restored
//! from a caller-supplied PID or from serialized state after a handover.
//!
//! Actuation is identity-bound on every platform: the target's PID, incarnation and
//! (where the kernel exposes it) start time are verified against the live process
//! before any stop is actuated, and a receipt is minted only for a stop this daemon
//! actuated. Positive *observation* of the stopped state is a second, stronger
//! guarantee that a platform may not be able to provide; the receipt records which
//! of the two it carries in `stop_observed`/`guarantee` rather than claiming the
//! stronger one. Classification and resume stay strict on every platform: only a
//! stop this daemon actuated and still owns is ever resumed, so a platform that cannot
//! observe a stop reports `Unknown` (never `External`, never auto-resume) instead of
//! guessing.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
#[path = "suspension/unix.rs"]
mod unix;
#[cfg(windows)]
#[path = "suspension/windows.rs"]
pub mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspensionSource {
    FerryxOwned,
    External,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuspensionTarget {
    pub pid: u32,
    pub incarnation: String,
    pub started_at_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActuationReceipt {
    pub pid: u32,
    pub incarnation: String,
    pub source: SuspensionSource,
    pub actuated_at_unix_ms: u64,
    /// True only when the backend positively observed the stopped state on the
    /// verified identity after actuation.
    pub stop_observed: bool,
    /// What this backend can prove about the stop it just actuated.
    pub guarantee: StopGuarantee,
}

/// How much of the suspension contract a platform backend can prove.
///
/// `IdentityBoundUnverifiedStop` is deliberately weaker than
/// `IdentityBoundObservedStop` and is reported verbatim on the receipt, so no
/// caller can read an unobserved stop as a proven one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopGuarantee {
    /// Identity-bound actuation with positive kernel observation of the stop.
    IdentityBoundObservedStop,
    /// Identity-bound actuation with an unverified stop: the platform has no way to
    /// observe the stopped state, so ownership rests on the pre-actuation identity
    /// check plus daemon-local bookkeeping.
    IdentityBoundUnverifiedStop,
}

#[derive(Debug, thiserror::Error)]
pub enum SuspensionError {
    #[error("identity-bound suspension is unsupported on this platform: {0}")]
    UnsupportedPlatform(&'static str),
    #[error("process {pid} identity does not match the suspension target")]
    IdentityMismatch { pid: u32 },
    #[error("process {pid} is gone")]
    ProcessGone { pid: u32 },
    #[error("{operation} failed: {source}")]
    PlatformCallFailure {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("process {pid} has no Ferryx-owned stop")]
    NotOwned { pid: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessIdentity {
    started_at_unix_ms: u64,
    // Preserve the kernel's full start discriminator, not just rounded milliseconds.
    start_token: u64,
    incarnation: Option<String>,
}

struct Observation {
    identity: ProcessIdentity,
    stopped: bool,
}

trait Process {
    fn observe(&self) -> Result<Observation, SuspensionError>;
    /// Must acknowledge a real stop on this pinned process before returning success.
    fn stop(&self) -> Result<(), SuspensionError>;
    fn resume(&self) -> Result<(), SuspensionError>;
}

struct OwnedStop {
    target: SuspensionTarget,
    identity: ProcessIdentity,
    receipt: ActuationReceipt,
}

#[derive(Default)]
struct Ownership {
    stops: HashMap<u32, OwnedStop>,
}

fn verify(target: &SuspensionTarget, identity: &ProcessIdentity) -> Result<(), SuspensionError> {
    if target.pid == 0
        || target.incarnation.is_empty()
        || target.started_at_unix_ms != Some(identity.started_at_unix_ms)
        || identity.incarnation.as_ref().is_some_and(|value| value != &target.incarnation)
    {
        return Err(SuspensionError::IdentityMismatch { pid: target.pid });
    }
    Ok(())
}

impl Ownership {
    fn stop(&mut self, target: &SuspensionTarget, process: &impl Process) -> Result<ActuationReceipt, SuspensionError> {
        let before = process.observe()?;
        verify(target, &before.identity)?;
        if before.stopped {
            // Sending SIGSTOP to an already stopped process must not claim its stop.
            return Err(SuspensionError::NotOwned { pid: target.pid });
        }
        self.stops.remove(&target.pid);
        process.stop()?;
        let after = process.observe()?;
        verify(target, &after.identity)?;
        if after.identity != before.identity || !after.stopped {
            return Err(SuspensionError::PlatformCallFailure {
                operation: "confirm stop",
                source: std::io::Error::other("stop was not observed on the verified identity"),
            });
        }
        let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| SuspensionError::PlatformCallFailure {
            operation: "timestamp stop",
            source: std::io::Error::other(error),
        })?;
        let receipt = ActuationReceipt {
            pid: target.pid,
            incarnation: target.incarnation.clone(),
            source: SuspensionSource::FerryxOwned,
            actuated_at_unix_ms: elapsed.as_millis().try_into().map_err(|error| SuspensionError::PlatformCallFailure {
                operation: "timestamp stop",
                source: std::io::Error::other(error),
            })?,
            // `Ownership::stop` returns only after re-observing the verified identity
            // in the stopped state, so this backend always carries the stronger one.
            stop_observed: true,
            guarantee: StopGuarantee::IdentityBoundObservedStop,
        };
        self.stops.insert(target.pid, OwnedStop { target: target.clone(), identity: after.identity, receipt: receipt.clone() });
        Ok(receipt)
    }

    fn classify(&mut self, target: &SuspensionTarget, process: &impl Process) -> Result<SuspensionSource, SuspensionError> {
        let observed = process.observe()?;
        verify(target, &observed.identity)?;
        if !observed.stopped {
            self.stops.remove(&target.pid);
            return Ok(SuspensionSource::Unknown);
        }
        match self.stops.get(&target.pid) {
            Some(owned) if owned.target == *target && owned.identity == observed.identity => Ok(owned.receipt.source),
            _ => {
                self.stops.remove(&target.pid);
                Ok(SuspensionSource::External)
            }
        }
    }

    fn resume(&mut self, target: &SuspensionTarget, process: &impl Process) -> Result<(), SuspensionError> {
        if self.classify(target, process)? != SuspensionSource::FerryxOwned {
            return Err(SuspensionError::NotOwned { pid: target.pid });
        }
        process.resume()?;
        self.stops.remove(&target.pid);
        Ok(())
    }
}

fn ownership() -> &'static parking_lot::Mutex<Ownership> {
    static OWNERSHIP: OnceLock<parking_lot::Mutex<Ownership>> = OnceLock::new();
    OWNERSHIP.get_or_init(|| parking_lot::Mutex::new(Ownership::default()))
}

pub fn stop_for_owned_suspension(target: &SuspensionTarget) -> Result<ActuationReceipt, SuspensionError> {
    #[cfg(unix)]
    { ownership().lock().stop(target, &unix::open(target)?) }
    #[cfg(windows)]
    { windows::stop_for_owned_suspension(target) }
    #[cfg(not(any(unix, windows)))]
    { let _ = target; Err(SuspensionError::UnsupportedPlatform("no suspension backend")) }
}

pub fn classify_stop_source(target: &SuspensionTarget) -> Result<SuspensionSource, SuspensionError> {
    #[cfg(unix)]
    { ownership().lock().classify(target, &unix::open(target)?) }
    #[cfg(windows)]
    { windows::classify_stop_source(target) }
    #[cfg(not(any(unix, windows)))]
    { let _ = target; Err(SuspensionError::UnsupportedPlatform("no suspension backend")) }
}

pub fn resume_owned(target: &SuspensionTarget) -> Result<(), SuspensionError> {
    #[cfg(unix)]
    { ownership().lock().resume(target, &unix::open(target)?) }
    #[cfg(windows)]
    { windows::resume_owned(target) }
    #[cfg(not(any(unix, windows)))]
    { let _ = target; Err(SuspensionError::UnsupportedPlatform("no suspension backend")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeProcess {
        start: Cell<u64>,
        stopped: Cell<bool>,
        fail_stop: bool,
        acknowledge: bool,
        resumes: Cell<usize>,
    }
    impl Process for FakeProcess {
        fn observe(&self) -> Result<Observation, SuspensionError> {
            Ok(Observation { identity: ProcessIdentity { started_at_unix_ms: self.start.get(), start_token: self.start.get(), incarnation: Some("pane-a".into()) }, stopped: self.stopped.get() })
        }
        fn stop(&self) -> Result<(), SuspensionError> {
            if self.fail_stop { return Err(SuspensionError::PlatformCallFailure { operation: "fake stop", source: std::io::Error::from_raw_os_error(1) }); }
            self.stopped.set(self.acknowledge);
            Ok(())
        }
        fn resume(&self) -> Result<(), SuspensionError> {
            self.resumes.set(self.resumes.get() + 1);
            self.stopped.set(false);
            Ok(())
        }
    }
    fn target() -> SuspensionTarget { SuspensionTarget { pid: 42, incarnation: "pane-a".into(), started_at_unix_ms: Some(100) } }
    fn process() -> FakeProcess { FakeProcess { start: Cell::new(100), stopped: Cell::new(false), fail_stop: false, acknowledge: true, resumes: Cell::new(0) } }

    #[test]
    fn receipt_requires_successful_acknowledged_actuation() {
        let mut ledger = Ownership::default();
        let mut process = process();
        process.fail_stop = true;
        assert!(ledger.stop(&target(), &process).is_err());
        assert!(ledger.stops.is_empty());
        process.fail_stop = false;
        process.acknowledge = false;
        assert!(ledger.stop(&target(), &process).is_err());
        assert!(ledger.stops.is_empty());
        process.acknowledge = true;
        let receipt = ledger.stop(&target(), &process).expect("acknowledged stop");
        assert_eq!(receipt.source, SuspensionSource::FerryxOwned);
        assert!(receipt.stop_observed);
        assert_eq!(receipt.guarantee, StopGuarantee::IdentityBoundObservedStop);
        assert_eq!(ledger.classify(&target(), &process).expect("classify"), SuspensionSource::FerryxOwned);
        ledger.resume(&target(), &process).expect("owned resume");
        assert_eq!(process.resumes.get(), 1);
    }

    #[test]
    fn reused_pid_and_changed_incarnation_cannot_claim_receipt() {
        let mut ledger = Ownership::default();
        let process = process();
        ledger.stop(&target(), &process).expect("stop");
        process.start.set(200);
        assert!(matches!(ledger.classify(&target(), &process), Err(SuspensionError::IdentityMismatch { .. })));
        let mut replacement = target();
        replacement.started_at_unix_ms = Some(200);
        assert_eq!(ledger.classify(&replacement, &process).expect("replacement"), SuspensionSource::External);
        replacement.incarnation = "pane-b".into();
        assert!(matches!(ledger.classify(&replacement, &process), Err(SuspensionError::IdentityMismatch { .. })));
    }

    #[test]
    fn external_stop_is_never_claimed_or_resumed() {
        let mut ledger = Ownership::default();
        let process = process();
        process.stopped.set(true);
        assert_eq!(ledger.classify(&target(), &process).expect("external"), SuspensionSource::External);
        assert!(matches!(ledger.stop(&target(), &process), Err(SuspensionError::NotOwned { .. })));
        assert!(matches!(ledger.resume(&target(), &process), Err(SuspensionError::NotOwned { .. })));
        assert_eq!(process.resumes.get(), 0);
    }

    #[test]
    fn identity_mismatch_refuses_resume() {
        let mut ledger = Ownership::default();
        let process = process();
        ledger.stop(&target(), &process).expect("stop");
        process.start.set(200);
        assert!(matches!(ledger.resume(&target(), &process), Err(SuspensionError::IdentityMismatch { .. })));
        assert_eq!(process.resumes.get(), 0);
    }

    #[test]
    fn observed_external_continue_revokes_ownership() {
        let mut ledger = Ownership::default();
        let process = process();
        ledger.stop(&target(), &process).expect("stop");
        process.stopped.set(false);
        assert_eq!(ledger.classify(&target(), &process).expect("running"), SuspensionSource::Unknown);
        process.stopped.set(true);
        assert_eq!(ledger.classify(&target(), &process).expect("restopped"), SuspensionSource::External);
    }
}
