//! Keeper-side session state.
//!
//! [`SessionState`] is the answer to "what does the keeper own, and who may talk to it": identity of
//! the child (`pid` plus the keeper generation that spawned it), the current epoch, the replay log's
//! bounds, and the focus/activity bookkeeping `OWNERSHIP_LIST` reports.
//!
//! It is a **snapshot**, not the live session. The keeper's live session owns the master descriptor
//! and the blocking reader thread; this type owns no descriptor and clones no PTY handles, so it can
//! be built fresh on every ownership query and handed to the wire layer without any of the keeper's
//! locks being held across serialization. `replay` is not copied for the same reason: the offsets and
//! counters are what the wire needs.
//!
//! Ownership is literal in this crate: the master belongs to whichever process holds it, and this
//! type only records which owner that is.

use serde::{Deserialize, Serialize};

/// Who owns a session's PTY. Persisted per session, not per process: flipping the feature flag routes
/// *new* spawns and leaves existing sessions with their original owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOwner {
    /// The keeper holds the master; the session survives a daemon crash.
    Keeper,
    /// The daemon holds the master directly; a daemon crash ends the session.
    Daemon,
}

impl SessionOwner {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keeper => "keeper",
            Self::Daemon => "daemon",
        }
    }
}

/// The keeper's answer to `OWNERSHIP_QUERY`/`OWNERSHIP_LIST` for one session.
///
/// The fields are public because this is a plain snapshot the wire layer serializes directly; the
/// accessor methods exist so call sites that only read a field do not have to know whether the
/// snapshot ever grows a derived value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionState {
    /// Session id, as the daemon named it.
    pub id: String,
    /// The keeper install generation that spawned it, so retention can ask whether a generation
    /// still holds anything.
    pub generation: String,
    /// Child pid. `None` when the platform could not report one.
    pub pid: Option<u32>,
    /// The newest epoch in force for this session.
    pub epoch: u64,
    /// Output offset a reattaching daemon resumes from.
    pub replay_head_offset: u64,
    /// Bytes the replay log still retains.
    pub replay_retained_bytes: u64,
    /// Lossy resets this session's replay log has performed.
    pub replay_resets: u64,
    /// Wall-clock milliseconds of the last attach, write or resize.
    pub last_activity_millis: u64,
    /// Whether the daemon last reported this session as focused.
    pub focused: bool,
    /// Which process holds the master.
    pub owner: SessionOwner,
    cols: u16,
    rows: u16,
}

impl SessionState {
    /// A snapshot built from the live session's identity and current size.
    ///
    /// `epoch`, the replay counters and the activity bookkeeping are filled in by the caller through
    /// [`SessionState::with_epoch`] and friends: the live session holds those behind its own locks,
    /// and reading them here would either take those locks in the wrong order or report a value the
    /// caller did not intend.
    pub fn new(
        id: impl Into<String>,
        generation: impl Into<String>,
        pid: Option<u32>,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self {
            id: id.into(),
            generation: generation.into(),
            pid,
            epoch: 0,
            replay_head_offset: 0,
            replay_retained_bytes: 0,
            replay_resets: 0,
            last_activity_millis: 0,
            focused: false,
            owner: SessionOwner::Keeper,
            cols,
            rows,
        }
    }

    pub fn with_epoch(mut self, epoch: u64) -> Self {
        self.epoch = epoch;
        self
    }

    pub fn with_replay(mut self, head_offset: u64, retained_bytes: u64, resets: u64) -> Self {
        self.replay_head_offset = head_offset;
        self.replay_retained_bytes = retained_bytes;
        self.replay_resets = resets;
        self
    }

    pub fn with_activity(mut self, last_activity_millis: u64, focused: bool) -> Self {
        self.last_activity_millis = last_activity_millis;
        self.focused = focused;
        self
    }

    pub fn with_owner(mut self, owner: SessionOwner) -> Self {
        self.owner = owner;
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The session's output head: the offset a reattaching daemon resumes from.
    pub fn sequence_offset(&self) -> u64 {
        self.replay_head_offset
    }

    pub fn cols(&self) -> u16 {
        self.cols
    }

    pub fn rows(&self) -> u16 {
        self.rows
    }

    pub fn size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    pub fn owner(&self) -> SessionOwner {
        self.owner
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn last_activity_millis(&self) -> u64 {
        self.last_activity_millis
    }

    /// Resets and activity counters reset with the session, not with the snapshot: this is only here
    /// so a snapshot built for a *new* session is recognisable as untouched.
    pub fn is_pristine(&self) -> bool {
        self.replay_head_offset == 0 && self.replay_retained_bytes == 0 && self.replay_resets == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SessionState {
        SessionState::new("s1", "gen-1", Some(4242), 80, 24)
    }

    #[test]
    fn a_fresh_snapshot_reports_identity_and_size_with_no_replay_yet() {
        let state = state();
        assert_eq!(state.id(), "s1");
        assert_eq!(state.generation(), "gen-1");
        assert_eq!(state.pid(), Some(4242));
        assert_eq!(state.size(), (80, 24));
        assert_eq!(state.owner(), SessionOwner::Keeper);
        assert!(state.is_pristine());
        assert!(!state.is_focused());
    }

    #[test]
    fn the_builder_setters_carry_the_live_values_a_query_reports() {
        let state = state()
            .with_epoch(3)
            .with_replay(11, 11, 2)
            .with_activity(1_700_000_000_000, true)
            .with_owner(SessionOwner::Daemon);
        assert_eq!(state.epoch(), 3);
        assert_eq!(state.sequence_offset(), 11, "the head is the resume offset");
        assert_eq!(state.replay_retained_bytes, 11);
        assert_eq!(state.replay_resets, 2);
        assert_eq!(state.last_activity_millis(), 1_700_000_000_000);
        assert!(state.is_focused());
        assert_eq!(state.owner().as_str(), "daemon");
        assert!(!state.is_pristine());
    }

    #[test]
    fn a_session_without_a_pid_still_answers_ownership() {
        let state = SessionState::new("s2", "gen-1", None, 132, 43);
        assert_eq!(state.pid(), None);
        assert_eq!(state.size(), (132, 43));
    }
}
