//! Per-session epoch fencing.
//!
//! Every attach to a session carries a monotonically increasing epoch. The keeper records the
//! newest epoch each session has been attached at; a read or write that presents anything older is
//! refused with [`KeeperErrorCode::EpochMismatch`], so a stalled daemon that wakes after a successor
//! attached cannot consume output or type into the PTY.
//!
//! An epoch newer than the newest seen is not usable until it [`EpochFence::fence`]s, and a stale
//! one is refused at that point too. That keeps "who may read this session" one explicit fact rather
//! than something a caller can infer from a number it happens to be holding.
//!
//! The fence is shared by every session because an epoch is only meaningful within one keeper: the
//! map is small (one entry per session), and one guard makes "newest per session" impossible to
//! read inconsistently.

use std::collections::HashMap;

use crate::error::{KeeperError, KeeperResult};

#[derive(Debug, Clone, Default)]
pub struct EpochFence {
    newest: HashMap<String, u64>,
}

impl EpochFence {
    pub fn new() -> Self {
        Self::default()
    }

    /// The newest epoch this keeper has attached or issued for `session_id`.
    pub fn newest(&self, session_id: &str) -> Option<u64> {
        self.newest.get(session_id).copied()
    }

    /// Admits an attach at `epoch`, which must be strictly newer than the newest one already seen
    /// for the session. Returns the epoch now in force.
    ///
    /// Epoch 0 is the daemon's "I hold no epoch" value on a first contact, so a session with no
    /// entry accepts it; anything at or below an epoch already seen is the stale-daemon case.
    pub fn fence(&mut self, session_id: &str, epoch: u64) -> KeeperResult<u64> {
        match self.newest.get(session_id).copied() {
            Some(newest) if epoch <= newest => Err(self.mismatch(session_id, epoch, newest)),
            _ => {
                self.newest.insert(session_id.to_string(), epoch);
                Ok(epoch)
            }
        }
    }

    /// The read and write gate: the presented epoch must be the newest one in force.
    pub fn verify(&self, session_id: &str, epoch: u64) -> KeeperResult<()> {
        match self.newest.get(session_id).copied() {
            Some(newest) if epoch == newest => Ok(()),
            Some(newest) => Err(self.mismatch(session_id, epoch, newest)),
            // No epoch was ever fenced for this session, so nothing may read or write it yet. The
            // session-not-found case is reported by the caller, which holds the registry.
            None => Err(self.mismatch(session_id, epoch, 0)),
        }
    }

    /// Reads are gated identically to writes; the distinct name lets a call site read as the
    /// operation it guards.
    pub fn verify_read(&self, session_id: &str, epoch: u64) -> KeeperResult<()> {
        self.verify(session_id, epoch)
    }

    pub fn verify_write(&self, session_id: &str, epoch: u64) -> KeeperResult<()> {
        self.verify(session_id, epoch)
    }

    /// Forgets a session. A retired session's epochs are meaningless, and keeping them would let a
    /// stale map grow without bound and refuse a fresh session that reused the id.
    pub fn forget(&mut self, session_id: &str) {
        self.newest.remove(session_id);
    }

    /// How many sessions currently carry a fence. Telemetry and leak checks.
    pub fn tracked(&self) -> usize {
        self.newest.len()
    }

    fn mismatch(&self, session_id: &str, presented: u64, newest: u64) -> KeeperError {
        KeeperError::epoch_mismatch(session_id, presented, newest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KeeperErrorCode;

    #[test]
    fn a_stale_epoch_is_refused_on_read_write_and_attach() {
        let mut fence = EpochFence::new();
        assert_eq!(fence.fence("s1", 1).expect("first attach"), 1);
        assert_eq!(fence.fence("s1", 2).expect("successor attaches"), 2);
        assert_eq!(fence.newest("s1"), Some(2));

        for refusal in [
            fence.verify_read("s1", 1),
            fence.verify_write("s1", 1),
            fence.fence("s1", 1).map(|_| ()),
        ] {
            let error = refusal.expect_err("epoch 1 is stale");
            assert_eq!(error.code, KeeperErrorCode::EpochMismatch);
            assert!(error.message.contains("older than attached epoch 2"));
        }
        assert_eq!(fence.newest("s1"), Some(2), "a refused attach does not advance the fence");
    }

    #[test]
    fn an_epoch_newer_than_the_fence_is_refused_until_it_attaches() {
        let mut fence = EpochFence::new();
        fence.fence("s1", 1).expect("first");
        let error = fence.verify("s1", 9).expect_err("9 never attached");
        assert_eq!(error.code, KeeperErrorCode::EpochMismatch);
        fence.fence("s1", 9).expect("attach at 9");
        fence.verify("s1", 9).expect("now readable");
    }

    #[test]
    fn a_session_with_no_fence_refuses_every_epoch() {
        let fence = EpochFence::new();
        assert_eq!(
            fence.verify("s1", 1).expect_err("nothing fenced yet").code,
            KeeperErrorCode::EpochMismatch
        );
        assert_eq!(fence.tracked(), 0);
    }

    #[test]
    fn the_first_daemon_is_fenced_out_once_a_successor_attaches() {
        let mut fence = EpochFence::new();
        fence.fence("s1", 1).expect("first daemon attaches");
        fence.verify("s1", 1).expect("first daemon reads");
        fence.fence("s1", 2).expect("successor takes over");
        assert_eq!(
            fence.verify("s1", 1).expect_err("stalled daemon").code,
            KeeperErrorCode::EpochMismatch
        );
        fence.verify("s1", 2).expect("the successor reads");
    }

    #[test]
    fn forgetting_a_session_lets_a_reused_id_fence_again() {
        let mut fence = EpochFence::new();
        fence.fence("s1", 7).expect("attach");
        fence.forget("s1");
        assert_eq!(fence.newest("s1"), None);
        assert_eq!(fence.tracked(), 0);
        fence.fence("s1", 1).expect("a fresh session fences from scratch");
    }

    #[test]
    fn sessions_are_fenced_independently() {
        let mut fence = EpochFence::new();
        fence.fence("s1", 5).expect("s1");
        fence.fence("s2", 1).expect("s2 has its own sequence");
        fence.verify("s1", 5).expect("s1 at 5");
        fence.verify("s2", 1).expect("s2 at 1");
        assert_eq!(fence.tracked(), 2);
    }
}
