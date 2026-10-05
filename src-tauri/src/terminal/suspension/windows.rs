//! Windows suspension backend.
//!
//! The supported Windows process APIs actuate a stop (`NtSuspendProcess`) and a
//! resume (`NtResumeProcess`) but cannot report the resulting state: the thread
//! suspend count is not readable through a documented interface, so a stopped
//! process is indistinguishable from a running one by any query this daemon can
//! make. The backend therefore proves two things and admits the third:
//!
//! * identity -- the PID and incarnation are checked against the daemon's own PTY
//!   registry before anything is actuated, so a reused or foreign PID is never
//!   signalled;
//! * actuation -- a receipt is minted only after the actuation call succeeded;
//! * observation -- NOT provable here. Every receipt carries `stop_observed: false`
//!   and `StopGuarantee::IdentityBoundUnverifiedStop`, and ownership rests on
//!   daemon-local bookkeeping for this daemon lifetime, never on serialized state or
//!   a caller-supplied PID.
//!
//! Classification stays strict in the safe direction: a stop this daemon actuated is
//! `FerryxOwned`, and anything else is `Unknown` -- never `External`, which would
//! claim a proof of someone else's stop that this platform cannot make. Only
//! `FerryxOwned` is ever resumed, so an unprovable stop is never auto-resumed.

use super::{ActuationReceipt, StopGuarantee, SuspensionError, SuspensionSource, SuspensionTarget};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The daemon's PTY registry is the only trustworthy answer to "is this PID the
/// incarnation this daemon spawned?", and this module cannot see it. The daemon
/// installs its lookup here at boot; until then every actuation is refused.
type OwnershipVerifier = dyn Fn(u32, &str) -> bool + Send + Sync;

static OWNERSHIP_VERIFIER: OnceLock<Box<OwnershipVerifier>> = OnceLock::new();

/// Install the daemon's ownership authority. Idempotent; the first install wins.
pub fn install_ownership_verifier(
    verifier: impl Fn(u32, &str) -> bool + Send + Sync + 'static,
) {
    let _ = OWNERSHIP_VERIFIER.set(Box::new(verifier));
}

fn verifier() -> Option<&'static OwnershipVerifier> {
    OWNERSHIP_VERIFIER.get().map(|verifier| &**verifier)
}

fn platform_failure(operation: &'static str, message: String) -> SuspensionError {
    SuspensionError::PlatformCallFailure {
        operation,
        source: std::io::Error::other(message),
    }
}

fn timestamp() -> Result<u64, SuspensionError> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| platform_failure("timestamp stop", error.to_string()))?;
    elapsed
        .as_millis()
        .try_into()
        .map_err(|error: std::num::TryFromIntError| platform_failure("timestamp stop", error.to_string()))
}

/// Identity gate. A target this daemon cannot attribute is refused, never signalled.
fn verified(
    verify: Option<&OwnershipVerifier>,
    target: &SuspensionTarget,
) -> Result<(), SuspensionError> {
    if target.pid == 0 || target.incarnation.is_empty() {
        return Err(SuspensionError::IdentityMismatch { pid: target.pid });
    }
    match verify {
        Some(verify) if verify(target.pid, target.incarnation.as_str()) => Ok(()),
        // No authority installed, or a foreign PID: refuse rather than actuate.
        _ => Err(SuspensionError::NotOwned { pid: target.pid }),
    }
}

fn actuate_stop(pid: u32) -> Result<(), SuspensionError> {
    crate::terminal::session::windows_suspend::suspend_process(pid)
        .map_err(|message| platform_failure("NtSuspendProcess", message))
}

fn actuate_resume(pid: u32) -> Result<(), SuspensionError> {
    crate::terminal::session::windows_suspend::resume_process(pid)
        .map_err(|message| platform_failure("NtResumeProcess", message))
}

struct OwnedStop {
    target: SuspensionTarget,
    receipt: ActuationReceipt,
}

/// Daemon-lifetime ownership ledger: the Windows counterpart of the ledger in
/// `suspension.rs`, and equally never persisted.
#[derive(Default)]
struct WindowsOwnership {
    stops: HashMap<u32, OwnedStop>,
}

impl WindowsOwnership {
    fn stop(
        &mut self,
        target: &SuspensionTarget,
        verify: Option<&OwnershipVerifier>,
        stop_process: impl FnOnce(u32) -> Result<(), SuspensionError>,
    ) -> Result<ActuationReceipt, SuspensionError> {
        verified(verify, target)?;
        stop_process(target.pid)?;
        let receipt = ActuationReceipt {
            pid: target.pid,
            incarnation: target.incarnation.clone(),
            source: SuspensionSource::FerryxOwned,
            actuated_at_unix_ms: timestamp()?,
            stop_observed: false,
            guarantee: StopGuarantee::IdentityBoundUnverifiedStop,
        };
        self.stops.insert(
            target.pid,
            OwnedStop {
                target: target.clone(),
                receipt: receipt.clone(),
            },
        );
        Ok(receipt)
    }

    fn classify(
        &mut self,
        target: &SuspensionTarget,
        verify: Option<&OwnershipVerifier>,
    ) -> Result<SuspensionSource, SuspensionError> {
        verified(verify, target)?;
        match self.stops.get(&target.pid) {
            Some(owned) if owned.target == *target => Ok(owned.receipt.source),
            Some(_) => {
                // A new incarnation owns this PID now; the stop is not ours to resume.
                self.stops.remove(&target.pid);
                Ok(SuspensionSource::Unknown)
            }
            None => Ok(SuspensionSource::Unknown),
        }
    }

    fn resume(
        &mut self,
        target: &SuspensionTarget,
        verify: Option<&OwnershipVerifier>,
        resume_process: impl FnOnce(u32) -> Result<(), SuspensionError>,
    ) -> Result<(), SuspensionError> {
        if self.classify(target, verify)? != SuspensionSource::FerryxOwned {
            return Err(SuspensionError::NotOwned { pid: target.pid });
        }
        resume_process(target.pid)?;
        self.stops.remove(&target.pid);
        Ok(())
    }
}

fn ownership() -> &'static Mutex<WindowsOwnership> {
    static OWNERSHIP: OnceLock<Mutex<WindowsOwnership>> = OnceLock::new();
    OWNERSHIP.get_or_init(|| Mutex::new(WindowsOwnership::default()))
}

pub(super) fn stop_for_owned_suspension(
    target: &SuspensionTarget,
) -> Result<ActuationReceipt, SuspensionError> {
    ownership().lock().stop(target, verifier(), actuate_stop)
}

pub(super) fn classify_stop_source(
    target: &SuspensionTarget,
) -> Result<SuspensionSource, SuspensionError> {
    ownership().lock().classify(target, verifier())
}

pub(super) fn resume_owned(target: &SuspensionTarget) -> Result<(), SuspensionError> {
    ownership().lock().resume(target, verifier(), actuate_resume)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> SuspensionTarget {
        SuspensionTarget {
            pid: 42,
            incarnation: "pane-a".into(),
            started_at_unix_ms: None,
        }
    }

    fn accepts(pid: u32, incarnation: &str) -> bool {
        pid == 42 && incarnation == "pane-a"
    }

    #[test]
    fn unverifiable_identity_is_refused_before_any_actuation() {
        let mut ledger = WindowsOwnership::default();
        let mut actuated = false;
        let result = ledger.stop(&target(), None, |_| {
            actuated = true;
            Ok(())
        });
        assert!(matches!(result, Err(SuspensionError::NotOwned { pid: 42 })));
        assert!(!actuated, "an unattributable target must never be signalled");

        let mut foreign = target();
        foreign.incarnation = "pane-b".into();
        let authority: &OwnershipVerifier = &accepts;
        assert!(matches!(
            ledger.stop(&foreign, Some(authority), |_| Ok(())),
            Err(SuspensionError::NotOwned { pid: 42 })
        ));
        assert!(matches!(
            ledger.stop(
                &SuspensionTarget { pid: 0, ..target() },
                Some(authority),
                |_| Ok(())
            ),
            Err(SuspensionError::IdentityMismatch { pid: 0 })
        ));
    }

    #[test]
    fn actuation_mints_a_receipt_that_admits_the_missing_observation() {
        let mut ledger = WindowsOwnership::default();
        let mut actuated = 0;
        let authority: &OwnershipVerifier = &accepts;
        let receipt = ledger
            .stop(&target(), Some(authority), |pid| {
                assert_eq!(pid, 42);
                actuated += 1;
                Ok(())
            })
            .expect("identity-bound actuation");
        assert_eq!(actuated, 1);
        assert_eq!(receipt.pid, 42);
        assert_eq!(receipt.incarnation, "pane-a");
        assert_eq!(receipt.source, SuspensionSource::FerryxOwned);
        assert!(!receipt.stop_observed);
        assert_eq!(receipt.guarantee, StopGuarantee::IdentityBoundUnverifiedStop);
    }

    #[test]
    fn a_failed_actuation_mints_no_receipt_and_claims_no_ownership() {
        let mut ledger = WindowsOwnership::default();
        let authority: &OwnershipVerifier = &accepts;
        let failed = ledger.stop(&target(), Some(authority), |_| {
            Err(platform_failure("NtSuspendProcess", "denied".into()))
        });
        assert!(matches!(
            failed,
            Err(SuspensionError::PlatformCallFailure { .. })
        ));
        assert!(ledger.stops.is_empty());
        assert_eq!(
            ledger.classify(&target(), Some(authority)).expect("classify"),
            SuspensionSource::Unknown
        );
    }

    #[test]
    fn only_an_owned_stop_is_resumed() {
        let mut ledger = WindowsOwnership::default();
        let authority: &OwnershipVerifier = &accepts;
        let mut resumed = 0;
        assert!(matches!(
            ledger.resume(&target(), Some(authority), |_| {
                resumed += 1;
                Ok(())
            }),
            Err(SuspensionError::NotOwned { pid: 42 })
        ));
        assert_eq!(resumed, 0);

        ledger
            .stop(&target(), Some(authority), |_| Ok(()))
            .expect("stop");
        ledger
            .resume(&target(), Some(authority), |_| {
                resumed += 1;
                Ok(())
            })
            .expect("owned resume");
        assert_eq!(resumed, 1);
        assert_eq!(
            ledger.classify(&target(), Some(authority)).expect("classify"),
            SuspensionSource::Unknown
        );
    }

    #[test]
    fn an_unprovable_stop_is_never_reported_as_external() {
        let mut ledger = WindowsOwnership::default();
        let authority: &OwnershipVerifier = &accepts;
        assert_eq!(
            ledger.classify(&target(), Some(authority)).expect("classify"),
            SuspensionSource::Unknown
        );
    }

    #[test]
    fn reused_pid_with_a_new_incarnation_revokes_the_owned_stop() {
        let mut ledger = WindowsOwnership::default();
        let authority: &OwnershipVerifier = &accepts;
        ledger
            .stop(&target(), Some(authority), |_| Ok(()))
            .expect("stop");
        let mut replacement = target();
        replacement.incarnation = "pane-b".into();
        let accepts_replacement = |pid: u32, incarnation: &str| pid == 42 && incarnation == "pane-b";
        let authority: &OwnershipVerifier = &accepts_replacement;
        assert_eq!(
            ledger
                .classify(&replacement, Some(authority))
                .expect("classify"),
            SuspensionSource::Unknown
        );
        assert!(ledger.stops.is_empty());
    }
}
