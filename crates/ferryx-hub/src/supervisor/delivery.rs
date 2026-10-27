//! The last owner check before a claimed dispatch is typed.
//!
//! A local PTY delivery may wait up to the pane-ready bound after its claim was confirmed
//! ([`super::confirm_claim_before_transmit`]). An owner pause, cancel or correction that
//! commits during that wait must still stop the bytes. Every owner hold and correction that
//! commits while a dispatch of the goal is `claimed` revokes that request here (before its
//! transaction commits; a revoke whose commit then fails only refuses a send, it never
//! allows one), and the transport asks [`check_before_typing`] synchronously inside the
//! pane's input step, right before the first byte. A revoked request is refused with a
//! pre-write code, so nothing is typed and the dispatcher releases the claim.
//!
//! The fence lives in this process: the daemon that owns the hub is also the one that types
//! into its local panes. A paired host waits for readiness on its own side and is not fenced
//! by this check.
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

/// Why a claimed send was refused right before typing: an owner hold or correction landed
/// after its claim.
pub const SEND_REVOKED: &str = "SEND_REVOKED";

static REVOKED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

fn revoked() -> std::sync::MutexGuard<'static, HashSet<String>> {
    REVOKED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Revokes every request in `request_ids`: none of them may be typed from now on. Owner holds,
/// corrections and lane pauses call it inside their write transaction.
pub fn revoke<I, S>(request_ids: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut set = revoked();
    for request_id in request_ids {
        set.insert(request_id.into());
    }
}

/// `Err(SEND_REVOKED)` when an owner hold or correction revoked `request_id` after it was
/// claimed. Synchronous on purpose: transports call it inside the pane's input step.
pub fn check_before_typing(request_id: &str) -> Result<(), &'static str> {
    if revoked().contains(request_id) {
        Err(SEND_REVOKED)
    } else {
        Ok(())
    }
}

/// Drops the fence entry of a request whose delivery is over (sent, refused or released).
pub fn forget(request_id: &str) {
    revoked().remove(request_id);
}

/// A typing fence for one delivery that only refuses before the first byte. The input step
/// asks it right before the first write and again before Enter; once the first ask passes,
/// bytes are in the composer, and withholding Enter would only leave the instruction typed
/// with an unknown outcome. So after the first `Ok` every later ask is `Ok`.
pub fn one_shot_fence(request_id: impl Into<String>) -> impl Fn() -> Result<(), &'static str> + Send + Sync {
    let request_id = request_id.into();
    let passed = std::sync::atomic::AtomicBool::new(false);
    move || {
        if passed.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        check_before_typing(&request_id)?;
        passed.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_one_shot_fence_refuses_only_before_the_first_byte() {
        let revoked_early = format!("fence-early-{}", uuid::Uuid::new_v4());
        revoke([revoked_early.clone()]);
        let early = one_shot_fence(revoked_early.clone());
        assert_eq!(early(), Err(SEND_REVOKED));
        assert_eq!(early(), Err(SEND_REVOKED), "still refused: nothing was typed");
        forget(&revoked_early);

        let revoked_late = format!("fence-late-{}", uuid::Uuid::new_v4());
        let late = one_shot_fence(revoked_late.clone());
        assert_eq!(late(), Ok(()), "the first byte is allowed");
        revoke([revoked_late.clone()]);
        assert_eq!(late(), Ok(()), "once typed, Enter is never withheld");
        assert_eq!(check_before_typing(&revoked_late), Err(SEND_REVOKED));
        forget(&revoked_late);
    }
}
