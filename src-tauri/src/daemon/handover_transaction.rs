//! Ferryx v5 Handover Transaction State Machine and Idempotency Ledger.
//!
//! Implements sections 5 and 14 of the Rolling Handover Session Ownership Transfer Design
//! (`ROLLING_HANDOVER_SESSION_OWNERSHIP_TRANSFER_DESIGN_2026-09-19.md`).
//!
//! # Architecture
//!
//! The handover transaction coordinates the atomic, single-writer ownership transfer
//! of live daemon resources (PTY master FDs, SSH bridge FDs, canonical listener/locks,
//! session metadata, and TerminalOutputHub ring-buffers) from the predecessor daemon to
//! the successor daemon.
//!
//! ## State Machine
//!
//! ```text
//!              prepare()           freeze()           start_transfer()
//!   [ Active ] --------> [ Preparing ] ------> [ Frozen ] ---------> [ Transferring ]
//!       ^                     |                   |                       |
//!       |                     |                   |                       |
//!       | rollback()          | rollback()        | rollback()            | rollback()
//!       +---------------------+-------------------+-----------------------+
//!       |
//!       |           mark_commit_ready()          commit(nonce)
//!       | [ Transferring ] ---------> [ CommitReady ] -------> [ Retired ] (terminal)
//!       |                                   |
//!       +------------- rollback() ----------+
//! ```
//!
//! Pre-commit states (`Preparing`, `Frozen`, `Transferring`, `CommitReady`) can safely
//! roll back to `Active` on any error, partial transfer, or disconnection, restoring
//! authoritative predecessor I/O and canonical operation.
//!
//! `Retired` is the terminal ownership point: once final `CommitAck` is verified,
//! ownership transfer is irreversible and the predecessor terminates without killing sessions.
//!
//! ## Idempotency & Frame Ledger
//!
//! Two layers of idempotency are enforced:
//! 1. **Request Idempotency**: Mutating handover requests carry a stable `client_request_id`
//!    and request `fingerprint`. Repeated requests with identical fingerprints reuse the
//!    transaction result; different fingerprints produce a conflict error.
//! 2. **Frame Idempotency**: Each transfer frame is identified by
//!    `(transfer_id, frame_sequence, session_ordinal, fd_roles, payload_digest)`.
//!    Duplicate delivery with identical identity replays the previously generated ACK
//!    (signaling the receiver to close duplicate received FDs). Conflicting content on the
//!    same sequence produces a protocol conflict error.

#![cfg(unix)]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Lifecycle states of a Ferryx v5 handover transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandoverState {
    /// Normal daemon operation; no handover in progress, or rolled back.
    Active,
    /// Handshake completed; negotiating inventory and preparing freeze.
    Preparing,
    /// Predecessor mutation/session I/O frozen; inventories locked.
    Frozen,
    /// Frame and descriptor transfer in progress.
    Transferring,
    /// All sessions/FDs transferred and verified in staging; awaiting commit challenge/ACK.
    CommitReady,
    /// Final commit ACK exchanged and verified; ownership transfer irreversible. Predecessor retired.
    Retired,
}

impl HandoverState {
    /// Returns true if this state is a pre-commit state capable of rolling back to `Active`.
    pub fn is_pre_commit(&self) -> bool {
        matches!(
            self,
            Self::Preparing | Self::Frozen | Self::Transferring | Self::CommitReady
        )
    }

    /// Returns true if this state is the irreversible post-commit terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Retired)
    }

    /// Checks whether transitioning from `self` to `target` is allowed by the protocol rules.
    pub fn can_transition_to(&self, target: HandoverState) -> bool {
        match (self, target) {
            // Forward progression
            (Self::Active, Self::Preparing) => true,
            (Self::Preparing, Self::Frozen) => true,
            (Self::Frozen, Self::Transferring) => true,
            (Self::Transferring, Self::CommitReady) => true,
            (Self::CommitReady, Self::Retired) => true,
            // Pre-commit rollbacks to Active
            (Self::Preparing, Self::Active) => true,
            (Self::Frozen, Self::Active) => true,
            (Self::Transferring, Self::Active) => true,
            (Self::CommitReady, Self::Active) => true,
            // All other transitions forbidden
            _ => false,
        }
    }
}

/// Kind of session transferred during handover.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Pty,
    DirectSsh,
    PairedDaemon,
}

/// Logical role of a file descriptor transferred across the handover socket via `SCM_RIGHTS`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "snake_case")]
pub enum FdRole {
    CanonicalListener,
    LegacyDaemonLock,
    PersistentDaemonLock,
    PtyMaster { session_id: String },
    SshControlStdin { session_id: String },
    SshControlStdout { session_id: String },
    SshControlStderr { session_id: String },
    SshReaderStdin { session_id: String },
    SshReaderStdout { session_id: String },
    SshReaderStderr { session_id: String },
    Custom { name: String, session_id: Option<String> },
}

impl FdRole {
    pub fn canonical_listener() -> Self {
        Self::CanonicalListener
    }

    pub fn legacy_daemon_lock() -> Self {
        Self::LegacyDaemonLock
    }

    pub fn persistent_daemon_lock() -> Self {
        Self::PersistentDaemonLock
    }

    pub fn pty_master(session_id: impl Into<String>) -> Self {
        Self::PtyMaster {
            session_id: session_id.into(),
        }
    }

    pub fn ssh_control_stdin(session_id: impl Into<String>) -> Self {
        Self::SshControlStdin {
            session_id: session_id.into(),
        }
    }

    pub fn ssh_control_stdout(session_id: impl Into<String>) -> Self {
        Self::SshControlStdout {
            session_id: session_id.into(),
        }
    }

    pub fn ssh_control_stderr(session_id: impl Into<String>) -> Self {
        Self::SshControlStderr {
            session_id: session_id.into(),
        }
    }

    pub fn ssh_reader_stdin(session_id: impl Into<String>) -> Self {
        Self::SshReaderStdin {
            session_id: session_id.into(),
        }
    }

    pub fn ssh_reader_stdout(session_id: impl Into<String>) -> Self {
        Self::SshReaderStdout {
            session_id: session_id.into(),
        }
    }

    pub fn ssh_reader_stderr(session_id: impl Into<String>) -> Self {
        Self::SshReaderStderr {
            session_id: session_id.into(),
        }
    }

    pub fn custom(name: impl Into<String>, session_id: Option<String>) -> Self {
        Self::Custom {
            name: name.into(),
            session_id,
        }
    }
}

/// An entry in the ordered session inventory declared before transfer begins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInventoryEntry {
    pub session_id: String,
    pub session_kind: SessionKind,
    pub expected_fd_roles: Vec<FdRole>,
}

impl SessionInventoryEntry {
    pub fn new(
        session_id: impl Into<String>,
        session_kind: SessionKind,
        expected_fd_roles: Vec<FdRole>,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            session_kind,
            expected_fd_roles,
        }
    }

    pub fn pty(session_id: impl Into<String>) -> Self {
        let sid = session_id.into();
        Self {
            expected_fd_roles: vec![FdRole::pty_master(&sid)],
            session_id: sid,
            session_kind: SessionKind::Pty,
        }
    }

    pub fn direct_ssh(session_id: impl Into<String>) -> Self {
        let sid = session_id.into();
        let expected_fd_roles = vec![
            FdRole::ssh_control_stdin(&sid),
            FdRole::ssh_control_stdout(&sid),
            FdRole::ssh_control_stderr(&sid),
            FdRole::ssh_reader_stdin(&sid),
            FdRole::ssh_reader_stdout(&sid),
            FdRole::ssh_reader_stderr(&sid),
        ];
        Self {
            session_id: sid,
            session_kind: SessionKind::DirectSsh,
            expected_fd_roles,
        }
    }

    pub fn paired_daemon(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            session_kind: SessionKind::PairedDaemon,
            expected_fd_roles: Vec::new(),
        }
    }
}

/// Frame identity: `(transferId, frameSequence, session ordinal, fd roles, payload digest)`.
///
/// Used for frame-level deduplication, replay of previous ACK, and conflict detection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameId {
    pub transfer_id: String,
    pub frame_sequence: u64,
    pub session_ordinal: Option<usize>,
    pub fd_roles: Vec<FdRole>,
    pub payload_digest: String,
}

impl FrameId {
    pub fn new(
        transfer_id: impl Into<String>,
        frame_sequence: u64,
        session_ordinal: Option<usize>,
        fd_roles: Vec<FdRole>,
        payload_digest: impl Into<String>,
    ) -> Self {
        Self {
            transfer_id: transfer_id.into(),
            frame_sequence,
            session_ordinal,
            fd_roles,
            payload_digest: payload_digest.into(),
        }
    }
}

/// Status recorded in a frame acknowledgment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameAckStatus {
    Accepted,
    DuplicateReplayed,
}

/// Acknowledgment payload returned upon receiving a frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameAck {
    pub transfer_id: String,
    pub frame_sequence: u64,
    pub status: FrameAckStatus,
    pub fd_roles_accepted: Vec<FdRole>,
    pub timestamp_ms: u64,
}

/// Outcome of processing an incoming frame against the ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum FrameReceiptOutcome {
    /// Newly accepted frame; caller should stage received descriptors and send this ACK.
    Accepted(FrameAck),
    /// Duplicate frame detected with identical identity; caller must close newly received
    /// duplicate descriptors and replay this prior ACK to the sender.
    DuplicateReplayAck(FrameAck),
}

impl FrameReceiptOutcome {
    pub fn ack(&self) -> &FrameAck {
        match self {
            Self::Accepted(ack) | Self::DuplicateReplayAck(ack) => ack,
        }
    }

    pub fn is_duplicate(&self) -> bool {
        matches!(self, Self::DuplicateReplayAck(_))
    }
}

/// Record of an acknowledged frame stored in the receipt ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameReceiptRecord {
    pub frame_id: FrameId,
    pub ack: FrameAck,
}

/// Receipt ledger tracking transferred frames, enforcing monotonic sequence,
/// detecting identical retries, and surfacing conflicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameReceiptLedger {
    transfer_id: String,
    records: BTreeMap<u64, FrameReceiptRecord>,
    next_expected_sequence: u64,
}

impl FrameReceiptLedger {
    pub fn new(transfer_id: impl Into<String>) -> Self {
        Self {
            transfer_id: transfer_id.into(),
            records: BTreeMap::new(),
            next_expected_sequence: 0,
        }
    }

    pub fn new_with_initial_sequence(
        transfer_id: impl Into<String>,
        initial_sequence: u64,
    ) -> Self {
        Self {
            transfer_id: transfer_id.into(),
            records: BTreeMap::new(),
            next_expected_sequence: initial_sequence,
        }
    }

    pub fn transfer_id(&self) -> &str {
        &self.transfer_id
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn next_expected_sequence(&self) -> u64 {
        self.next_expected_sequence
    }

    pub fn get_record(&self, sequence: u64) -> Option<&FrameReceiptRecord> {
        self.records.get(&sequence)
    }

    pub fn records(&self) -> impl Iterator<Item = &FrameReceiptRecord> {
        self.records.values()
    }

    /// Records a frame using the system clock for acknowledgment timestamp.
    pub fn record_frame(&mut self, frame: FrameId) -> Result<FrameReceiptOutcome, HandoverError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.record_frame_with_timestamp(frame, now_ms)
    }

    /// Records a frame with an explicit timestamp.
    ///
    /// Duplicate frames with identical `FrameId` replay the previous ACK.
    /// Same sequence with differing content produces `HandoverError::FrameConflict`.
    pub fn record_frame_with_timestamp(
        &mut self,
        frame: FrameId,
        now_ms: u64,
    ) -> Result<FrameReceiptOutcome, HandoverError> {
        if frame.transfer_id != self.transfer_id {
            return Err(HandoverError::TransferIdMismatch {
                expected: self.transfer_id.clone(),
                actual: frame.transfer_id,
            });
        }

        let seq = frame.frame_sequence;

        // Check if this frame sequence was already recorded
        if let Some(existing) = self.records.get(&seq) {
            if existing.frame_id == frame {
                // Identical frame identity: duplicate detected! Replay prior ACK.
                let mut replayed_ack = existing.ack.clone();
                replayed_ack.status = FrameAckStatus::DuplicateReplayed;
                return Ok(FrameReceiptOutcome::DuplicateReplayAck(replayed_ack));
            } else {
                // Same frame sequence with conflicting content
                return Err(HandoverError::FrameConflict {
                    sequence: seq,
                    recorded: Box::new(existing.frame_id.clone()),
                    incoming: Box::new(frame),
                });
            }
        }

        // Allow 0 or 1 as starting sequence on first frame if default 0
        if self.records.is_empty() && self.next_expected_sequence == 0 && seq == 1 {
            self.next_expected_sequence = 1;
        }

        if seq != self.next_expected_sequence {
            return Err(HandoverError::FrameOutOfOrder {
                expected: self.next_expected_sequence,
                actual: seq,
            });
        }

        let ack = FrameAck {
            transfer_id: self.transfer_id.clone(),
            frame_sequence: seq,
            status: FrameAckStatus::Accepted,
            fd_roles_accepted: frame.fd_roles.clone(),
            timestamp_ms: now_ms,
        };

        let record = FrameReceiptRecord {
            frame_id: frame,
            ack: ack.clone(),
        };

        self.records.insert(seq, record);
        self.next_expected_sequence = seq + 1;

        Ok(FrameReceiptOutcome::Accepted(ack))
    }

    /// Collects all unique FD roles acknowledged across all accepted frames.
    pub fn all_received_fd_roles(&self) -> Vec<FdRole> {
        let mut roles = Vec::new();
        for record in self.records.values() {
            for role in &record.frame_id.fd_roles {
                if !roles.contains(role) {
                    roles.push(role.clone());
                }
            }
        }
        roles
    }
}

/// Summary result of a completed or in-flight handover transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverTransactionResult {
    pub transfer_id: String,
    pub client_request_id: String,
    pub state: HandoverState,
    pub sessions_count: usize,
    pub frames_received: usize,
    pub manifest_digest: Option<String>,
    pub commit_nonce: Option<String>,
}

/// The core handover transaction entity.
///
/// Encapsulates:
/// - transfer ID, client request ID, predecessor & successor PID/epoch
/// - ordered session inventory
/// - expected FD-role inventory
/// - frame receipt ledger
/// - commit nonce
/// - state machine and rollback tracking
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverTransaction {
    pub transfer_id: String,
    pub client_request_id: String,
    pub predecessor_pid: u32,
    pub predecessor_epoch: u64,
    pub successor_pid: u32,
    pub successor_epoch: u64,
    pub session_inventory: Vec<SessionInventoryEntry>,
    pub expected_fd_roles: Vec<FdRole>,
    pub frame_receipt_ledger: FrameReceiptLedger,
    pub commit_nonce: Option<String>,
    pub state: HandoverState,
    pub fingerprint: String,
    pub manifest_digest: Option<String>,
    pub rollback_reason: Option<String>,
    pub result: Option<HandoverTransactionResult>,
}

impl HandoverTransaction {
    /// Creates a new handover transaction in the `Active` state.
    pub fn new(
        transfer_id: impl Into<String>,
        client_request_id: impl Into<String>,
        predecessor_pid: u32,
        predecessor_epoch: u64,
        successor_pid: u32,
        successor_epoch: u64,
        session_inventory: Vec<SessionInventoryEntry>,
        expected_fd_roles: Vec<FdRole>,
        fingerprint: impl Into<String>,
    ) -> Self {
        let transfer_id = transfer_id.into();
        let ledger = FrameReceiptLedger::new(transfer_id.clone());
        Self {
            transfer_id,
            client_request_id: client_request_id.into(),
            predecessor_pid,
            predecessor_epoch,
            successor_pid,
            successor_epoch,
            session_inventory,
            expected_fd_roles,
            frame_receipt_ledger: ledger,
            commit_nonce: None,
            state: HandoverState::Active,
            fingerprint: fingerprint.into(),
            manifest_digest: None,
            rollback_reason: None,
            result: None,
        }
    }

    /// Convenience constructor with empty inventories.
    pub fn new_simple(
        transfer_id: impl Into<String>,
        client_request_id: impl Into<String>,
        predecessor_pid: u32,
        predecessor_epoch: u64,
        successor_pid: u32,
        successor_epoch: u64,
        fingerprint: impl Into<String>,
    ) -> Self {
        Self::new(
            transfer_id,
            client_request_id,
            predecessor_pid,
            predecessor_epoch,
            successor_pid,
            successor_epoch,
            Vec::new(),
            Vec::new(),
            fingerprint,
        )
    }

    pub fn with_session(mut self, session: SessionInventoryEntry) -> Self {
        self.session_inventory.push(session);
        self
    }

    pub fn with_expected_fd_role(mut self, role: FdRole) -> Self {
        self.expected_fd_roles.push(role);
        self
    }

    // --- Accessor methods ---

    pub fn transfer_id(&self) -> &str {
        &self.transfer_id
    }

    pub fn client_request_id(&self) -> &str {
        &self.client_request_id
    }

    pub fn predecessor_pid(&self) -> u32 {
        self.predecessor_pid
    }

    pub fn predecessor_epoch(&self) -> u64 {
        self.predecessor_epoch
    }

    pub fn successor_pid(&self) -> u32 {
        self.successor_pid
    }

    pub fn successor_epoch(&self) -> u64 {
        self.successor_epoch
    }

    pub fn session_inventory(&self) -> &[SessionInventoryEntry] {
        &self.session_inventory
    }

    pub fn ordered_session_inventory(&self) -> &[SessionInventoryEntry] {
        &self.session_inventory
    }

    pub fn expected_fd_roles(&self) -> &[FdRole] {
        &self.expected_fd_roles
    }

    pub fn expected_fd_role_inventory(&self) -> &[FdRole] {
        &self.expected_fd_roles
    }

    pub fn frame_receipt_ledger(&self) -> &FrameReceiptLedger {
        &self.frame_receipt_ledger
    }

    pub fn commit_nonce(&self) -> Option<&str> {
        self.commit_nonce.as_deref()
    }

    pub fn state(&self) -> HandoverState {
        self.state
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn manifest_digest(&self) -> Option<&str> {
        self.manifest_digest.as_deref()
    }

    pub fn rollback_reason(&self) -> Option<&str> {
        self.rollback_reason.as_deref()
    }

    /// Computes or retrieves the cached transaction execution result.
    pub fn result(&self) -> HandoverTransactionResult {
        if let Some(res) = &self.result {
            res.clone()
        } else {
            self.compute_result()
        }
    }

    pub fn compute_result(&self) -> HandoverTransactionResult {
        HandoverTransactionResult {
            transfer_id: self.transfer_id.clone(),
            client_request_id: self.client_request_id.clone(),
            state: self.state,
            sessions_count: self.session_inventory.len(),
            frames_received: self.frame_receipt_ledger.len(),
            manifest_digest: self.manifest_digest.clone(),
            commit_nonce: self.commit_nonce.clone(),
        }
    }

    /// Returns true if every declared expected FD role has been received in the ledger.
    pub fn are_all_expected_roles_received(&self) -> bool {
        let received = self.frame_receipt_ledger.all_received_fd_roles();
        self.expected_fd_roles
            .iter()
            .all(|role| received.contains(role))
    }

    // --- State machine transition methods ---

    /// Transitions `Active -> Preparing`.
    pub fn prepare(&mut self) -> Result<(), HandoverError> {
        self.transition_to(HandoverState::Preparing)
    }

    /// Transitions `Preparing -> Frozen`, locking inventories and recording manifest digest.
    pub fn freeze(&mut self, manifest_digest: Option<String>) -> Result<(), HandoverError> {
        if !self.state.can_transition_to(HandoverState::Frozen) {
            return Err(HandoverError::InvalidTransition {
                from: self.state,
                to: HandoverState::Frozen,
                reason: format!(
                    "cannot freeze transaction in state {:?}; expected Preparing",
                    self.state
                ),
            });
        }
        self.manifest_digest = manifest_digest;
        self.state = HandoverState::Frozen;
        Ok(())
    }

    /// Transitions `Frozen -> Transferring`.
    pub fn start_transfer(&mut self) -> Result<(), HandoverError> {
        self.transition_to(HandoverState::Transferring)
    }

    /// Records an incoming frame in the ledger during the `Transferring` state.
    pub fn record_frame(&mut self, frame: FrameId) -> Result<FrameReceiptOutcome, HandoverError> {
        if self.state != HandoverState::Transferring {
            return Err(HandoverError::InvalidState {
                current: self.state,
                expected: HandoverState::Transferring,
            });
        }
        self.frame_receipt_ledger.record_frame(frame)
    }

    /// Transitions `Transferring -> CommitReady`, storing the one-time commit challenge nonce.
    pub fn mark_commit_ready(
        &mut self,
        commit_nonce: impl Into<String>,
    ) -> Result<(), HandoverError> {
        if !self.state.can_transition_to(HandoverState::CommitReady) {
            return Err(HandoverError::InvalidTransition {
                from: self.state,
                to: HandoverState::CommitReady,
                reason: format!(
                    "cannot mark commit ready in state {:?}; expected Transferring",
                    self.state
                ),
            });
        }
        let nonce = commit_nonce.into();
        if nonce.trim().is_empty() {
            return Err(HandoverError::EmptyCommitNonce);
        }
        self.commit_nonce = Some(nonce);
        self.state = HandoverState::CommitReady;
        Ok(())
    }

    /// Transitions `CommitReady -> Retired`, verifying the provided commit nonce.
    ///
    /// This is the irreversible ownership transfer point.
    pub fn commit(
        &mut self,
        commit_nonce: &str,
    ) -> Result<HandoverTransactionResult, HandoverError> {
        if !self.state.can_transition_to(HandoverState::Retired) {
            return Err(HandoverError::InvalidTransition {
                from: self.state,
                to: HandoverState::Retired,
                reason: format!(
                    "cannot commit transaction in state {:?}; expected CommitReady",
                    self.state
                ),
            });
        }
        match &self.commit_nonce {
            Some(expected) if expected == commit_nonce => {
                self.state = HandoverState::Retired;
                let res = self.compute_result();
                self.result = Some(res.clone());
                Ok(res)
            }
            Some(expected) => Err(HandoverError::CommitNonceMismatch {
                expected: expected.clone(),
                actual: commit_nonce.to_string(),
            }),
            None => Err(HandoverError::MissingCommitNonce),
        }
    }

    /// Rolls back the transaction from any pre-commit state (`Preparing`, `Frozen`,
    /// `Transferring`, `CommitReady`) to `Active`.
    ///
    /// Rollback from `Retired` is strictly forbidden.
    /// Rollback when already `Active` is rejected as an invalid transition.
    pub fn rollback(&mut self, reason: impl Into<String>) -> Result<(), HandoverError> {
        if self.state == HandoverState::Retired {
            return Err(HandoverError::AlreadyCommitted);
        }
        if self.state == HandoverState::Active {
            return Err(HandoverError::InvalidTransition {
                from: HandoverState::Active,
                to: HandoverState::Active,
                reason: "transaction is already in Active state; cannot rollback".to_string(),
            });
        }
        self.rollback_reason = Some(reason.into());
        self.state = HandoverState::Active;
        Ok(())
    }

    /// Generic state transition validator and applicator.
    pub fn transition_to(&mut self, target: HandoverState) -> Result<(), HandoverError> {
        if !self.state.can_transition_to(target) {
            return Err(HandoverError::InvalidTransition {
                from: self.state,
                to: target,
                reason: format!(
                    "transition from {:?} to {:?} is not permitted",
                    self.state, target
                ),
            });
        }

        if target == HandoverState::Active {
            return self.rollback("transition_to(Active)");
        }

        if target == HandoverState::Retired {
            let res = self.compute_result();
            self.result = Some(res);
        }

        self.state = target;
        Ok(())
    }

    /// Checks transaction-level idempotency against `client_request_id` and `fingerprint`.
    ///
    /// Returns:
    /// - `Ok(true)` if identical (`client_request_id` and `fingerprint` match).
    /// - `Err(FingerprintConflict)` if `client_request_id` matches but fingerprint differs.
    /// - `Err(ClientRequestIdMismatch)` if `client_request_id` does not match.
    pub fn check_idempotency(
        &self,
        client_request_id: &str,
        fingerprint: &str,
    ) -> Result<bool, HandoverError> {
        if self.client_request_id != client_request_id {
            return Err(HandoverError::ClientRequestIdMismatch {
                expected: self.client_request_id.clone(),
                actual: client_request_id.to_string(),
            });
        }
        if self.fingerprint != fingerprint {
            return Err(HandoverError::FingerprintConflict {
                client_request_id: client_request_id.to_string(),
                recorded_fingerprint: self.fingerprint.clone(),
                incoming_fingerprint: fingerprint.to_string(),
            });
        }
        Ok(true)
    }
}

/// Registration outcome for idempotent transactions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IdempotentResult {
    /// Transaction newly registered.
    New,
    /// Idempotent retry: existing transaction result replayed.
    Reused(HandoverTransactionResult),
}

/// Central idempotency ledger managing handover transactions.
///
/// Guarantees:
/// - Same `client_request_id` + same `fingerprint` reuses the transaction result.
/// - Same `client_request_id` + different `fingerprint` produces `HandoverError::FingerprintConflict`.
/// - Lookup by either `client_request_id` or `transfer_id`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverIdempotencyLedger {
    transactions: HashMap<String, HandoverTransaction>,
    transfer_to_client: HashMap<String, String>,
}

impl HandoverIdempotencyLedger {
    pub fn new() -> Self {
        Self {
            transactions: HashMap::new(),
            transfer_to_client: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    pub fn get(&self, client_request_id: &str) -> Option<&HandoverTransaction> {
        self.transactions.get(client_request_id)
    }

    pub fn get_mut(&mut self, client_request_id: &str) -> Option<&mut HandoverTransaction> {
        self.transactions.get_mut(client_request_id)
    }

    pub fn get_by_transfer_id(&self, transfer_id: &str) -> Option<&HandoverTransaction> {
        let client_req_id = self.transfer_to_client.get(transfer_id)?;
        self.transactions.get(client_req_id)
    }

    pub fn get_mut_by_transfer_id(
        &mut self,
        transfer_id: &str,
    ) -> Option<&mut HandoverTransaction> {
        let client_req_id = self.transfer_to_client.get(transfer_id)?.clone();
        self.transactions.get_mut(&client_req_id)
    }

    /// Registers a new transaction or reuses an existing transaction if `client_request_id`
    /// and `fingerprint` match.
    ///
    /// - Returns `Ok(IdempotentResult::New)` if newly registered.
    /// - Returns `Ok(IdempotentResult::Reused(result))` on identical retry.
    /// - Returns `Err(HandoverError::FingerprintConflict)` if fingerprint differs.
    pub fn register_or_reuse(
        &mut self,
        transaction: HandoverTransaction,
    ) -> Result<IdempotentResult, HandoverError> {
        let client_req_id = transaction.client_request_id.clone();
        let fingerprint = transaction.fingerprint.clone();

        if let Some(existing) = self.transactions.get(&client_req_id) {
            if existing.fingerprint != fingerprint {
                return Err(HandoverError::FingerprintConflict {
                    client_request_id: client_req_id,
                    recorded_fingerprint: existing.fingerprint.clone(),
                    incoming_fingerprint: fingerprint,
                });
            }
            return Ok(IdempotentResult::Reused(existing.result()));
        }

        let transfer_id = transaction.transfer_id.clone();
        self.transfer_to_client
            .insert(transfer_id, client_req_id.clone());
        self.transactions.insert(client_req_id, transaction);
        Ok(IdempotentResult::New)
    }

    /// Checks whether a request with `client_request_id` has been recorded.
    ///
    /// - `Ok(None)`: not yet seen.
    /// - `Ok(Some(result))`: seen with matching fingerprint, returns replayed result.
    /// - `Err(FingerprintConflict)`: seen with different fingerprint.
    pub fn check_idempotency(
        &self,
        client_request_id: &str,
        fingerprint: &str,
    ) -> Result<Option<HandoverTransactionResult>, HandoverError> {
        if let Some(existing) = self.transactions.get(client_request_id) {
            if existing.fingerprint != fingerprint {
                return Err(HandoverError::FingerprintConflict {
                    client_request_id: client_request_id.to_string(),
                    recorded_fingerprint: existing.fingerprint.clone(),
                    incoming_fingerprint: fingerprint.to_string(),
                });
            }
            return Ok(Some(existing.result()));
        }
        Ok(None)
    }

    /// Records a frame for the specified transfer via the ledger, with duplicate-detect
    /// and replay-previous-ACK semantics.
    pub fn record_frame(&mut self, frame: FrameId) -> Result<FrameReceiptOutcome, HandoverError> {
        let transfer_id = frame.transfer_id.clone();
        let client_req_id = self
            .transfer_to_client
            .get(&transfer_id)
            .cloned()
            .ok_or_else(|| HandoverError::UnknownTransferId(transfer_id.clone()))?;

        let tx = self
            .transactions
            .get_mut(&client_req_id)
            .ok_or_else(|| HandoverError::UnknownTransferId(transfer_id))?;

        tx.record_frame(frame)
    }
}

/// Errors originating from handover state transitions, ledger tracking, or protocol mismatches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum HandoverError {
    InvalidTransition {
        from: HandoverState,
        to: HandoverState,
        reason: String,
    },
    InvalidState {
        current: HandoverState,
        expected: HandoverState,
    },
    AlreadyCommitted,
    FingerprintConflict {
        client_request_id: String,
        recorded_fingerprint: String,
        incoming_fingerprint: String,
    },
    ClientRequestIdMismatch {
        expected: String,
        actual: String,
    },
    UnknownTransferId(String),
    TransferIdMismatch {
        expected: String,
        actual: String,
    },
    FrameConflict {
        sequence: u64,
        recorded: Box<FrameId>,
        incoming: Box<FrameId>,
    },
    FrameOutOfOrder {
        expected: u64,
        actual: u64,
    },
    CommitNonceMismatch {
        expected: String,
        actual: String,
    },
    MissingCommitNonce,
    EmptyCommitNonce,
    Other(String),
}

impl fmt::Display for HandoverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransition { from, to, reason } => {
                write!(
                    f,
                    "invalid handover transition from {from:?} to {to:?}: {reason}"
                )
            }
            Self::InvalidState { current, expected } => {
                write!(
                    f,
                    "invalid handover state: current is {current:?}, expected {expected:?}"
                )
            }
            Self::AlreadyCommitted => {
                write!(
                    f,
                    "handover transaction has already committed/retired; ownership transfer is irreversible"
                )
            }
            Self::FingerprintConflict {
                client_request_id,
                recorded_fingerprint,
                incoming_fingerprint,
            } => {
                write!(
                    f,
                    "clientRequestId '{client_request_id}' fingerprint conflict: recorded '{recorded_fingerprint}', incoming '{incoming_fingerprint}'"
                )
            }
            Self::ClientRequestIdMismatch { expected, actual } => {
                write!(
                    f,
                    "clientRequestId mismatch: expected '{expected}', got '{actual}'"
                )
            }
            Self::UnknownTransferId(id) => {
                write!(f, "unknown transferId '{id}'")
            }
            Self::TransferIdMismatch { expected, actual } => {
                write!(
                    f,
                    "transferId mismatch: expected '{expected}', got '{actual}'"
                )
            }
            Self::FrameConflict {
                sequence,
                recorded,
                incoming,
            } => {
                write!(
                    f,
                    "frame sequence {sequence} conflict: recorded payload digest '{}', incoming payload digest '{}'",
                    recorded.payload_digest, incoming.payload_digest
                )
            }
            Self::FrameOutOfOrder { expected, actual } => {
                write!(
                    f,
                    "frame out of order: expected sequence {expected}, got {actual}"
                )
            }
            Self::CommitNonceMismatch { expected, actual } => {
                write!(
                    f,
                    "commit nonce mismatch: expected '{expected}', got '{actual}'"
                )
            }
            Self::MissingCommitNonce => {
                write!(f, "commit nonce was not set on transaction")
            }
            Self::EmptyCommitNonce => {
                write!(f, "commit nonce cannot be empty")
            }
            Self::Other(msg) => write!(f, "handover error: {msg}"),
        }
    }
}

impl std::error::Error for HandoverError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_transaction(client_id: &str, fingerprint: &str) -> HandoverTransaction {
        let pty_session = SessionInventoryEntry::pty("pty-sess-1");
        let ssh_session = SessionInventoryEntry::direct_ssh("ssh-sess-2");
        let paired_session = SessionInventoryEntry::paired_daemon("paired-sess-3");

        let expected_roles = vec![
            FdRole::canonical_listener(),
            FdRole::legacy_daemon_lock(),
            FdRole::persistent_daemon_lock(),
            FdRole::pty_master("pty-sess-1"),
            FdRole::ssh_control_stdin("ssh-sess-2"),
            FdRole::ssh_control_stdout("ssh-sess-2"),
            FdRole::ssh_control_stderr("ssh-sess-2"),
            FdRole::ssh_reader_stdin("ssh-sess-2"),
            FdRole::ssh_reader_stdout("ssh-sess-2"),
            FdRole::ssh_reader_stderr("ssh-sess-2"),
        ];

        HandoverTransaction::new(
            "transfer-alpha-001",
            client_id,
            1001,
            1726700000000,
            2002,
            1726700005000,
            vec![pty_session, ssh_session, paired_session],
            expected_roles,
            fingerprint,
        )
    }

    fn sample_frame(seq: u64, roles: Vec<FdRole>, digest: &str) -> FrameId {
        FrameId::new("transfer-alpha-001", seq, Some(0), roles, digest)
    }

    #[test]
    fn test_happy_lifecycle_through_every_state() {
        // Initial state: Active
        let mut tx = sample_transaction("req-lifecycle-01", "fp-valid-01");
        assert_eq!(tx.state(), HandoverState::Active);
        assert!(!tx.state().is_pre_commit());
        assert!(!tx.state().is_terminal());

        // 1. Active -> Preparing
        tx.prepare().expect("prepare should succeed");
        assert_eq!(tx.state(), HandoverState::Preparing);
        assert!(tx.state().is_pre_commit());

        // 2. Preparing -> Frozen
        let digest = Some("manifest-sha256-abcdef".to_string());
        tx.freeze(digest.clone()).expect("freeze should succeed");
        assert_eq!(tx.state(), HandoverState::Frozen);
        assert_eq!(tx.manifest_digest(), digest.as_deref());
        assert!(tx.state().is_pre_commit());

        // 3. Frozen -> Transferring
        tx.start_transfer()
            .expect("start_transfer should succeed");
        assert_eq!(tx.state(), HandoverState::Transferring);
        assert!(tx.state().is_pre_commit());

        // Send frames during Transferring
        let f0 = sample_frame(0, vec![FdRole::canonical_listener()], "digest-f0");
        let out0 = tx.record_frame(f0).expect("frame 0 should succeed");
        assert!(!out0.is_duplicate());
        assert_eq!(out0.ack().frame_sequence, 0);
        assert_eq!(out0.ack().status, FrameAckStatus::Accepted);

        let f1 = sample_frame(1, vec![FdRole::pty_master("pty-sess-1")], "digest-f1");
        let out1 = tx.record_frame(f1).expect("frame 1 should succeed");
        assert!(!out1.is_duplicate());
        assert_eq!(out1.ack().frame_sequence, 1);

        // 4. Transferring -> CommitReady
        let nonce = "commit-nonce-xyz-777";
        tx.mark_commit_ready(nonce)
            .expect("mark_commit_ready should succeed");
        assert_eq!(tx.state(), HandoverState::CommitReady);
        assert_eq!(tx.commit_nonce(), Some(nonce));
        assert!(tx.state().is_pre_commit());

        // 5. CommitReady -> Retired (irreversible commit)
        let result = tx.commit(nonce).expect("commit should succeed");
        assert_eq!(tx.state(), HandoverState::Retired);
        assert_eq!(result.state, HandoverState::Retired);
        assert_eq!(result.frames_received, 2);
        assert_eq!(result.sessions_count, 3);
        assert_eq!(result.manifest_digest, digest);
        assert_eq!(result.commit_nonce, Some(nonce.to_string()));
        assert!(tx.state().is_terminal());
        assert!(!tx.state().is_pre_commit());
    }

    #[test]
    fn test_every_invalid_transition_rejected() {
        let all_states = [
            HandoverState::Active,
            HandoverState::Preparing,
            HandoverState::Frozen,
            HandoverState::Transferring,
            HandoverState::CommitReady,
            HandoverState::Retired,
        ];

        for &from in &all_states {
            for &to in &all_states {
                let allowed = match (from, to) {
                    (HandoverState::Active, HandoverState::Preparing) => true,
                    (HandoverState::Preparing, HandoverState::Frozen) => true,
                    (HandoverState::Frozen, HandoverState::Transferring) => true,
                    (HandoverState::Transferring, HandoverState::CommitReady) => true,
                    (HandoverState::CommitReady, HandoverState::Retired) => true,
                    // Rollbacks
                    (HandoverState::Preparing, HandoverState::Active) => true,
                    (HandoverState::Frozen, HandoverState::Active) => true,
                    (HandoverState::Transferring, HandoverState::Active) => true,
                    (HandoverState::CommitReady, HandoverState::Active) => true,
                    _ => false,
                };

                let mut tx = sample_transaction("req-matrix", "fp-matrix");
                tx.state = from;
                if from == HandoverState::CommitReady {
                    tx.commit_nonce = Some("test-nonce".to_string());
                }

                let res = tx.transition_to(to);
                if allowed {
                    assert!(
                        res.is_ok(),
                        "expected transition {from:?} -> {to:?} to be OK, got {res:?}"
                    );
                    assert_eq!(tx.state(), to);
                } else {
                    assert!(
                        res.is_err(),
                        "expected transition {from:?} -> {to:?} to be REJECTED"
                    );
                }
            }
        }
    }

    #[test]
    fn test_pre_commit_rollback_to_active_from_every_pre_commit_state() {
        // Rollback from Preparing
        let mut tx_prep = sample_transaction("req-rb-1", "fp-rb-1");
        tx_prep.prepare().expect("prepare");
        assert_eq!(tx_prep.state(), HandoverState::Preparing);
        tx_prep
            .rollback("failure during handshake")
            .expect("rollback");
        assert_eq!(tx_prep.state(), HandoverState::Active);
        assert_eq!(
            tx_prep.rollback_reason(),
            Some("failure during handshake")
        );

        // Rollback from Frozen
        let mut tx_frozen = sample_transaction("req-rb-2", "fp-rb-2");
        tx_frozen.prepare().expect("prepare");
        tx_frozen.freeze(None).expect("freeze");
        assert_eq!(tx_frozen.state(), HandoverState::Frozen);
        tx_frozen
            .rollback("freeze timeout reached")
            .expect("rollback");
        assert_eq!(tx_frozen.state(), HandoverState::Active);
        assert_eq!(
            tx_frozen.rollback_reason(),
            Some("freeze timeout reached")
        );

        // Rollback from Transferring
        let mut tx_xfer = sample_transaction("req-rb-3", "fp-rb-3");
        tx_xfer.prepare().expect("prepare");
        tx_xfer.freeze(None).expect("freeze");
        tx_xfer.start_transfer().expect("transfer");
        assert_eq!(tx_xfer.state(), HandoverState::Transferring);
        tx_xfer
            .rollback("socket disconnected mid-transfer")
            .expect("rollback");
        assert_eq!(tx_xfer.state(), HandoverState::Active);
        assert_eq!(
            tx_xfer.rollback_reason(),
            Some("socket disconnected mid-transfer")
        );

        // Rollback from CommitReady
        let mut tx_ready = sample_transaction("req-rb-4", "fp-rb-4");
        tx_ready.prepare().expect("prepare");
        tx_ready.freeze(None).expect("freeze");
        tx_ready.start_transfer().expect("transfer");
        tx_ready.mark_commit_ready("nonce-1").expect("ready");
        assert_eq!(tx_ready.state(), HandoverState::CommitReady);
        tx_ready
            .rollback("staging verification rejected")
            .expect("rollback");
        assert_eq!(tx_ready.state(), HandoverState::Active);
        assert_eq!(
            tx_ready.rollback_reason(),
            Some("staging verification rejected")
        );
    }

    #[test]
    fn test_rollback_from_retired_rejected() {
        let mut tx = sample_transaction("req-term-rb", "fp-term");
        tx.prepare().unwrap();
        tx.freeze(None).unwrap();
        tx.start_transfer().unwrap();
        tx.mark_commit_ready("nonce-term").unwrap();
        tx.commit("nonce-term").unwrap();
        assert_eq!(tx.state(), HandoverState::Retired);

        let err = tx
            .rollback("attempt post-commit rollback")
            .expect_err("post-commit rollback must fail");
        assert_eq!(err, HandoverError::AlreadyCommitted);
    }

    #[test]
    fn test_rollback_from_active_rejected() {
        let mut tx = sample_transaction("req-active-rb", "fp-active");
        assert_eq!(tx.state(), HandoverState::Active);

        let err = tx
            .rollback("rollback while active")
            .expect_err("rollback while active must fail");
        assert!(matches!(
            err,
            HandoverError::InvalidTransition {
                from: HandoverState::Active,
                to: HandoverState::Active,
                ..
            }
        ));
    }

    #[test]
    fn test_idempotent_retry_replays_result() {
        let mut ledger = HandoverIdempotencyLedger::new();
        let tx = sample_transaction("req-idemp-1", "fp-shared-1234");

        // First registration: New
        let reg = ledger
            .register_or_reuse(tx.clone())
            .expect("first registration succeeds");
        assert_eq!(reg, IdempotentResult::New);

        // Advance transaction to CommitReady
        let stored_tx = ledger.get_mut("req-idemp-1").expect("stored");
        stored_tx.prepare().unwrap();
        stored_tx
            .freeze(Some("digest-manifest".to_string()))
            .unwrap();
        stored_tx.start_transfer().unwrap();
        stored_tx.mark_commit_ready("nonce-idemp").unwrap();
        let expected_result = stored_tx.commit("nonce-idemp").unwrap();

        // Idempotent retry with same client_request_id + same fingerprint
        let retry_tx = sample_transaction("req-idemp-1", "fp-shared-1234");
        let retry_outcome = ledger
            .register_or_reuse(retry_tx)
            .expect("idempotent retry succeeds");

        match retry_outcome {
            IdempotentResult::Reused(replayed_result) => {
                assert_eq!(replayed_result, expected_result);
                assert_eq!(replayed_result.state, HandoverState::Retired);
                assert_eq!(
                    replayed_result.commit_nonce,
                    Some("nonce-idemp".to_string())
                );
            }
            IdempotentResult::New => panic!("expected Reused, got New"),
        }

        // Also test check_idempotency helper
        let checked = ledger
            .check_idempotency("req-idemp-1", "fp-shared-1234")
            .expect("idempotency check succeeds");
        assert_eq!(checked, Some(expected_result));
    }

    #[test]
    fn test_fingerprint_conflict_rejected() {
        let mut ledger = HandoverIdempotencyLedger::new();
        let tx1 = sample_transaction("req-conflict-1", "fp-original-hash");
        ledger
            .register_or_reuse(tx1)
            .expect("registration succeeds");

        // Retry with same client_request_id but DIFFERENT fingerprint
        let tx_conflicting = sample_transaction("req-conflict-1", "fp-DIFFERENT-hash");
        let err = ledger
            .register_or_reuse(tx_conflicting)
            .expect_err("fingerprint mismatch must be rejected");

        match err {
            HandoverError::FingerprintConflict {
                client_request_id,
                recorded_fingerprint,
                incoming_fingerprint,
            } => {
                assert_eq!(client_request_id, "req-conflict-1");
                assert_eq!(recorded_fingerprint, "fp-original-hash");
                assert_eq!(incoming_fingerprint, "fp-DIFFERENT-hash");
            }
            other => panic!("expected FingerprintConflict, got {other:?}"),
        }

        // check_idempotency helper also rejects conflicting fingerprint
        let check_err = ledger
            .check_idempotency("req-conflict-1", "fp-DIFFERENT-hash")
            .expect_err("check_idempotency must fail on conflicting fingerprint");
        assert!(matches!(check_err, HandoverError::FingerprintConflict { .. }));
    }

    #[test]
    fn test_duplicate_frame_detected_and_prior_ack_replayed() {
        let mut tx = sample_transaction("req-frame-dup", "fp-frame");
        tx.prepare().unwrap();
        tx.freeze(None).unwrap();
        tx.start_transfer().unwrap();

        let frame = sample_frame(0, vec![FdRole::canonical_listener()], "payload-digest-0");

        // First delivery: Accepted
        let outcome1 = tx
            .record_frame(frame.clone())
            .expect("first delivery succeeds");
        assert!(!outcome1.is_duplicate());
        let ack1 = outcome1.ack();
        assert_eq!(ack1.frame_sequence, 0);
        assert_eq!(ack1.status, FrameAckStatus::Accepted);
        assert_eq!(ack1.fd_roles_accepted, vec![FdRole::canonical_listener()]);

        // Duplicate delivery with IDENTICAL content: DuplicateReplayAck
        let outcome2 = tx
            .record_frame(frame.clone())
            .expect("duplicate delivery succeeds");
        assert!(outcome2.is_duplicate());
        let ack2 = outcome2.ack();
        assert_eq!(ack2.frame_sequence, 0);
        assert_eq!(ack2.status, FrameAckStatus::DuplicateReplayed);
        assert_eq!(ack2.fd_roles_accepted, ack1.fd_roles_accepted);

        // Ensure ledger length did not increase on duplicate
        assert_eq!(tx.frame_receipt_ledger().len(), 1);
    }

    #[test]
    fn test_frame_conflict_different_content_rejected() {
        let mut tx = sample_transaction("req-frame-conf", "fp-conf");
        tx.prepare().unwrap();
        tx.freeze(None).unwrap();
        tx.start_transfer().unwrap();

        let original_frame =
            sample_frame(0, vec![FdRole::canonical_listener()], "payload-digest-ORIGINAL");
        tx.record_frame(original_frame.clone()).unwrap();

        // Conflict 1: Same sequence (0), but different payload digest
        let tampered_digest =
            sample_frame(0, vec![FdRole::canonical_listener()], "payload-digest-TAMPERED");
        let err1 = tx
            .record_frame(tampered_digest)
            .expect_err("tampered payload digest must be rejected");
        assert!(matches!(
            err1,
            HandoverError::FrameConflict { sequence: 0, .. }
        ));

        // Conflict 2: Same sequence (0), but different FD roles
        let tampered_roles = sample_frame(
            0,
            vec![FdRole::pty_master("pty-sess-1")],
            "payload-digest-ORIGINAL",
        );
        let err2 = tx
            .record_frame(tampered_roles)
            .expect_err("differing FD roles must be rejected");
        assert!(matches!(
            err2,
            HandoverError::FrameConflict { sequence: 0, .. }
        ));
    }

    #[test]
    fn test_frame_out_of_order_rejected() {
        let mut tx = sample_transaction("req-order", "fp-order");
        tx.prepare().unwrap();
        tx.freeze(None).unwrap();
        tx.start_transfer().unwrap();

        // First frame seq 0
        let f0 = sample_frame(0, vec![FdRole::canonical_listener()], "d0");
        tx.record_frame(f0).unwrap();

        // Frame seq 5 received when seq 1 was expected
        let f5 = sample_frame(5, vec![FdRole::pty_master("p1")], "d5");
        let err = tx
            .record_frame(f5)
            .expect_err("out of order sequence must be rejected");

        match err {
            HandoverError::FrameOutOfOrder { expected, actual } => {
                assert_eq!(expected, 1);
                assert_eq!(actual, 5);
            }
            other => panic!("expected FrameOutOfOrder, got {other:?}"),
        }
    }

    #[test]
    fn test_frame_transfer_id_mismatch_rejected() {
        let mut tx = sample_transaction("req-txid", "fp-txid");
        tx.prepare().unwrap();
        tx.freeze(None).unwrap();
        tx.start_transfer().unwrap();

        let foreign_frame = FrameId::new("transfer-FOREIGN", 0, None, vec![], "d");
        let err = tx
            .record_frame(foreign_frame)
            .expect_err("foreign transfer_id must be rejected");

        assert!(matches!(err, HandoverError::TransferIdMismatch { .. }));
    }

    #[test]
    fn test_frame_rejected_when_not_in_transferring_state() {
        let mut tx = sample_transaction("req-pre-xfer", "fp-pre");
        let frame = sample_frame(0, vec![], "d");

        // Active state
        assert!(matches!(
            tx.record_frame(frame.clone()),
            Err(HandoverError::InvalidState {
                current: HandoverState::Active,
                expected: HandoverState::Transferring
            })
        ));

        // Preparing state
        tx.prepare().unwrap();
        assert!(matches!(
            tx.record_frame(frame.clone()),
            Err(HandoverError::InvalidState {
                current: HandoverState::Preparing,
                expected: HandoverState::Transferring
            })
        ));

        // Frozen state
        tx.freeze(None).unwrap();
        assert!(matches!(
            tx.record_frame(frame.clone()),
            Err(HandoverError::InvalidState {
                current: HandoverState::Frozen,
                expected: HandoverState::Transferring
            })
        ));
    }

    #[test]
    fn test_commit_nonce_validation() {
        let mut tx = sample_transaction("req-nonce", "fp-nonce");
        tx.prepare().unwrap();
        tx.freeze(None).unwrap();
        tx.start_transfer().unwrap();

        // Empty nonce rejected
        let empty_err = tx
            .mark_commit_ready("   ")
            .expect_err("empty commit nonce must be rejected");
        assert_eq!(empty_err, HandoverError::EmptyCommitNonce);

        // Valid nonce
        tx.mark_commit_ready("secret-nonce-12345").unwrap();

        // Nonce mismatch on commit
        let mismatch_err = tx
            .commit("wrong-nonce")
            .expect_err("wrong nonce must be rejected");
        assert!(matches!(
            mismatch_err,
            HandoverError::CommitNonceMismatch { .. }
        ));

        // Matching nonce succeeds
        let res = tx.commit("secret-nonce-12345").expect("commit succeeds");
        assert_eq!(res.state, HandoverState::Retired);
    }

    #[test]
    fn test_expected_fd_roles_tracking() {
        let mut tx = sample_transaction("req-roles", "fp-roles");
        assert!(!tx.are_all_expected_roles_received());

        tx.prepare().unwrap();
        tx.freeze(None).unwrap();
        tx.start_transfer().unwrap();

        // Expected roles in sample_transaction:
        // canonical_listener, legacy_daemon_lock, persistent_daemon_lock,
        // pty_master(pty-sess-1), and 6 SSH stdio roles for ssh-sess-2
        let f0 = sample_frame(
            0,
            vec![
                FdRole::canonical_listener(),
                FdRole::legacy_daemon_lock(),
                FdRole::persistent_daemon_lock(),
            ],
            "d0",
        );
        tx.record_frame(f0).unwrap();
        assert!(!tx.are_all_expected_roles_received());

        let f1 = sample_frame(1, vec![FdRole::pty_master("pty-sess-1")], "d1");
        tx.record_frame(f1).unwrap();
        assert!(!tx.are_all_expected_roles_received());

        let f2 = sample_frame(
            2,
            vec![
                FdRole::ssh_control_stdin("ssh-sess-2"),
                FdRole::ssh_control_stdout("ssh-sess-2"),
                FdRole::ssh_control_stderr("ssh-sess-2"),
                FdRole::ssh_reader_stdin("ssh-sess-2"),
                FdRole::ssh_reader_stdout("ssh-sess-2"),
                FdRole::ssh_reader_stderr("ssh-sess-2"),
            ],
            "d2",
        );
        tx.record_frame(f2).unwrap();

        // All 10 expected roles are now satisfied
        assert!(tx.are_all_expected_roles_received());
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let mut tx = sample_transaction("req-serde", "fp-serde");
        tx.prepare().unwrap();
        tx.freeze(Some("digest-123".to_string())).unwrap();
        tx.start_transfer().unwrap();
        tx.record_frame(sample_frame(0, vec![FdRole::canonical_listener()], "d0"))
            .unwrap();

        let json = serde_json::to_string_pretty(&tx).expect("serialize transaction");
        let deserialized: HandoverTransaction =
            serde_json::from_str(&json).expect("deserialize transaction");

        assert_eq!(tx, deserialized);
        assert_eq!(deserialized.state(), HandoverState::Transferring);
        assert_eq!(deserialized.frame_receipt_ledger().len(), 1);
        assert_eq!(deserialized.manifest_digest(), Some("digest-123"));
    }
}
