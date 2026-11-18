//! One daemon-owned durable journal. All methods are blocking; callers use run_blocking.
use super::machine_protocol::{Operation, OperationOutcome};
use crate::agent_control::events::{JournalBatch, ObservationHistory};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub device_id: String,
    pub request_id: String,
    pub kind: String,
    pub digest: String,
    pub resource: String,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    pub status: u16,
    pub operation: Operation,
    #[serde(default)]
    pub outcome_summary: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Store {
    version: u32,
    records: BTreeMap<String, Record>,
    #[serde(default)]
    sessions: BTreeMap<String, MachineSession>,
    #[serde(default)]
    session_revision: u64,
    /// Which journal schema step this file is at (`crate::agent_control::pending_input`). A file
    /// written before the field existed reads as `0` and is lifted to the current step in memory
    /// when it is loaded, then written out at that step by the next persist. Nothing has to be
    /// rewritten on a read-only open.
    #[serde(default)]
    schema: u32,
    /// Open and answered user-input requests an agent pane asked, keyed by `prompt_id`. See
    /// `crate::agent_control::pending_input` for the commit-intent/deliver/receipt protocol.
    /// Absent from a file written before step 55.
    #[serde(default)]
    pending_inputs: BTreeMap<String, crate::agent_control::pending_input::PendingInput>,
    /// The agent observer's durable tail: the bounded rows `replay(after)` serves, each stamped
    /// with the epoch that issued its sequence. This is the store of record for observation
    /// history; there is no second one. Absent in journals written before it existed.
    #[serde(default, skip_serializing_if = "ObservationHistory::is_empty")]
    observation_history: ObservationHistory,
}

/// Lift a store read from disk to the journal schema step this build knows.
///
/// A JSON journal has no ALTER TABLE, so the step is stamped in memory at load and persisted by
/// the next write; every field a later step added carries `#[serde(default)]`, so an older file
/// still parses. See `journal_migrations/0055_pending_input_requests.md`.
fn lift_schema(store: &mut Store) {
    store.schema = store
        .schema
        .max(crate::agent_control::pending_input::schema_step());
}

/// Durable ownership survives output-hub cleanup and journal result compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSession {
    pub creator_device: String,
    pub session: super::machine_protocol::Session,
    pub exit: Option<super::machine_protocol::ExitMetadata>,
    /// The state and screen times the agent observer last measured, kept so a restarted daemon
    /// does not restart them. Absent in journals written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_stamps: Option<crate::agent_control::events::SessionStamps>,
}
#[cfg(test)]
type JournalProbe = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

pub struct MachineOperationJournal {
    path: PathBuf,
    state: parking_lot::Mutex<Result<Store, String>>,
    #[cfg(test)]
    probe: parking_lot::RwLock<Option<JournalProbe>>,
}
pub enum Begin {
    New,
    Existing(Box<Record>),
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}
fn key(device: &str, request: &str) -> String {
    serde_json::to_string(&(device, request)).expect("string tuple")
}
impl MachineOperationJournal {
    pub(crate) fn recover_catalog(&self, receipt: &Record) -> Result<(), String> {
        let pending = self
            .reconcile(&receipt.device_id, &receipt.request_id)?
            .ok_or("MACHINE_SERVICE_UNAVAILABLE")?;
        if pending.digest != receipt.digest
            || pending.kind != receipt.kind
            || pending.resource != receipt.resource
        {
            return Err("MACHINE_SERVICE_UNAVAILABLE".into());
        }
        if matches!(
            pending.operation,
            Operation::Pending { .. } | Operation::OutcomeUnknown { .. }
        ) {
            let Operation::Completed { outcome, .. } = &receipt.operation else {
                return Err("MACHINE_SERVICE_UNAVAILABLE".into());
            };
            self.complete(
                &receipt.device_id,
                &receipt.request_id,
                receipt.status,
                outcome.as_ref().clone(),
            )?;
        }
        Ok(())
    }
    pub(crate) fn catalog_receipt(
        &self,
        device: &str,
        request: &str,
        status: u16,
        outcome: OperationOutcome,
    ) -> Result<Record, String> {
        let mut record = self
            .reconcile(device, request)?
            .ok_or("OPERATION_NOT_FOUND")?;
        if !matches!(record.operation, Operation::Pending { .. }) {
            return Err("REQUEST_CONFLICT".into());
        }
        record.status = status;
        record.operation = Operation::Completed {
            request_id: request.into(),
            outcome: Box::new(outcome),
        };
        Ok(record)
    }
    pub(crate) fn open(path: PathBuf) -> Self {
        let state = match std::fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store {
                version: 1,
                records: BTreeMap::new(),
                sessions: BTreeMap::new(),
                session_revision: 0,
                schema: crate::agent_control::pending_input::schema_step(),
                pending_inputs: BTreeMap::new(),
                observation_history: ObservationHistory::default(),
            }),
            Err(_) => Err("MACHINE_SERVICE_UNAVAILABLE".into()),
            Ok(bytes) => serde_json::from_slice::<Store>(&bytes)
                .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".into())
                .and_then(|mut s| {
                    if s.version != 1 {
                        return Err("MACHINE_SERVICE_UNAVAILABLE".into());
                    }
                    lift_schema(&mut s);
                    for r in s.records.values_mut() {
                        if matches!(r.operation, Operation::Pending { .. }) {
                            r.operation = Operation::OutcomeUnknown {
                                request_id: r.request_id.clone(),
                            };
                        }
                    }
                    Ok(s)
                }),
        };
        Self {
            path,
            state: parking_lot::Mutex::new(state),
            #[cfg(test)]
            probe: parking_lot::RwLock::new(None),
        }
    }
    fn refresh_locked(&self, state: &mut Result<Store, String>) -> Result<std::fs::File, String> {
        state.as_ref().map_err(Clone::clone)?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let lock = options
            .open(self.path.with_extension("tx.lock"))
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        lock.lock().map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let mut current: Store =
                    serde_json::from_slice(&bytes).map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
                if current.version != 1 {
                    return Err("MACHINE_SERVICE_UNAVAILABLE".into());
                }
                lift_schema(&mut current);
                let previous = state.as_ref().map_err(Clone::clone)?;
                for (key, record) in &mut current.records {
                    if matches!(record.operation, Operation::Pending { .. })
                        && previous.records.get(key).is_some_and(|prior| {
                            matches!(prior.operation, Operation::OutcomeUnknown { .. })
                        })
                    {
                        record.operation = Operation::OutcomeUnknown {
                            request_id: record.request_id.clone(),
                        };
                    }
                }
                *state = Ok(current);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("MACHINE_SERVICE_UNAVAILABLE".into()),
        }
        Ok(lock)
    }

    fn persist(&self, state: &mut Result<Store, String>, candidate: Store) -> Result<(), String> {
        #[cfg(test)]
        self.probe("persist");
        let result = super::auth::write_private_json(&self.path, &candidate)
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string());
        #[cfg(unix)]
        let result = result.and_then(|()| {
            std::fs::File::open(self.path.parent().expect("journal parent"))
                .and_then(|f| f.sync_all())
                .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string())
        });
        match result {
            Ok(()) => {
                *state = Ok(candidate);
                Ok(())
            }
            Err(e) => {
                *state = Err(e.clone());
                Err(e)
            }
        }
    }
    pub fn begin(
        &self,
        device_id: &str,
        request_id: &str,
        kind: &str,
        digest: &str,
        resource: &str,
    ) -> Result<Begin, String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut s = state.as_ref().map_err(Clone::clone)?.clone();
        let k = key(device_id, request_id);
        if let Some(r) = s.records.get(&k) {
            if r.digest != digest || r.kind != kind {
                return Err("REQUEST_CONFLICT".into());
            }
            return Ok(Begin::Existing(Box::new(r.clone())));
        }
        let time = now();
        let mut tombstones = s
            .records
            .values()
            .filter(|r| matches!(r.operation, Operation::ResultExpired { .. }))
            .count();
        for r in s.records.values_mut() {
            if matches!(r.operation, Operation::Completed { .. })
                && r.completed_at
                    .is_some_and(|t| time.saturating_sub(t) >= 7 * 86400)
                && tombstones < 100_000
            {
                r.operation = Operation::ResultExpired {
                    request_id: r.request_id.clone(),
                };
                tombstones += 1;
            }
        }
        if s.records.len() - tombstones >= 10_000 || tombstones >= 100_000 {
            return Err("CAPACITY_EXCEEDED".into());
        }
        s.records.insert(
            k,
            Record {
                device_id: device_id.into(),
                request_id: request_id.into(),
                kind: kind.into(),
                digest: digest.into(),
                resource: resource.into(),
                created_at: time,
                completed_at: None,
                status: 202,
                outcome_summary: None,
                operation: Operation::Pending {
                    request_id: request_id.into(),
                },
            },
        );
        self.persist(&mut state, s)?;
        Ok(Begin::New)
    }
    pub fn reconcile(&self, device_id: &str, request_id: &str) -> Result<Option<Record>, String> {
        #[cfg(test)]
        self.probe("reconcile");
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        Ok(state
            .as_ref()
            .map_err(Clone::clone)?
            .records
            .get(&key(device_id, request_id))
            .cloned())
    }
    pub(crate) fn sessions(&self) -> Result<Vec<MachineSession>, String> {
        #[cfg(test)]
        self.probe("sessions");
        Ok(self
            .state
            .lock()
            .as_ref()
            .map_err(Clone::clone)?
            .sessions
            .values()
            .cloned()
            .collect())
    }
    pub(crate) fn session_revision(&self) -> Result<crate::scoped_contracts::Epoch, String> {
        Ok(crate::scoped_contracts::Epoch(
            self.state
                .lock()
                .as_ref()
                .map_err(Clone::clone)?
                .session_revision,
        ))
    }
    /// Includes unacknowledged spawns, fencing mirror admission before metadata publication.
    pub(crate) fn owns_session(&self, id: &str) -> bool {
        let state = self.state.lock();
        let Ok(store) = state.as_ref() else {
            return true;
        };
        store.sessions.contains_key(id)
            || store.records.values().any(|record| {
                record.kind == "createSession"
                    && serde_json::from_str::<super::machine_protocol::RemoteTerminalTarget>(
                        &record.resource,
                    )
                    .is_ok_and(|target| target.session_id == id)
            })
    }
    /// Ownership and the original create receipt share one durable replacement.
    pub(crate) fn commit_spawn(
        &self,
        device: &str,
        request: &str,
        session: MachineSession,
    ) -> Result<(), String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let record = candidate
            .records
            .get_mut(&key(device, request))
            .ok_or("OPERATION_NOT_FOUND")?;
        if record.kind != "createSession"
            || record.resource != serde_json::to_string(&session.session.target).expect("target")
            || !matches!(
                record.operation,
                Operation::Pending { .. } | Operation::OutcomeUnknown { .. }
            )
        {
            return Err("REQUEST_CONFLICT".into());
        }
        record.status = 201;
        record.completed_at = Some(now());
        record.outcome_summary = Some("session".into());
        record.operation = Operation::Completed {
            request_id: request.into(),
            outcome: Box::new(OperationOutcome::Session {
                session: session.session.clone(),
            }),
        };
        candidate
            .sessions
            .insert(session.session.target.session_id.clone(), session);
        candidate.session_revision = candidate
            .session_revision
            .checked_add(1)
            .ok_or("CAPACITY_EXCEEDED")?;
        self.persist(&mut state, candidate)
    }
    pub(crate) fn session(&self, id: &str) -> Result<Option<MachineSession>, String> {
        Ok(self
            .state
            .lock()
            .as_ref()
            .map_err(Clone::clone)?
            .sessions
            .get(id)
            .cloned())
    }
    /// The persisted agent stamps of every session that has them, by session id.
    pub(crate) fn agent_stamps(
        &self,
    ) -> Result<BTreeMap<String, crate::agent_control::events::SessionStamps>, String> {
        Ok(self
            .state
            .lock()
            .as_ref()
            .map_err(Clone::clone)?
            .sessions
            .iter()
            .filter_map(|(id, session)| Some((id.clone(), session.agent_stamps.clone()?)))
            .collect())
    }
    /// Stores the newest stamps of each named session in one write. A session this journal does
    /// not own (a desktop pane, or one that ended) is skipped; it changes nothing else, so no
    /// session revision moves and no machine event is owed.
    pub(crate) fn record_agent_stamps(
        &self,
        stamps: &BTreeMap<String, crate::agent_control::events::SessionStamps>,
    ) -> Result<(), String> {
        self.record_journal(JournalBatch {
            stamps: stamps.clone(),
            observations: Vec::new(),
        })
    }

    /// The durable observation tail a previous run persisted, if any.
    pub(crate) fn observation_history(&self) -> Result<ObservationHistory, String> {
        #[cfg(test)]
        self.probe("observation_history");
        Ok(self
            .state
            .lock()
            .as_ref()
            .map_err(Clone::clone)?
            .observation_history
            .clone())
    }

    /// Applies one batch from the observation writer: the newest stamps per session and the
    /// observations to append, oldest first. One writer applies both, so the journal stays the
    /// single store of record; a second writer would need a precedence rule nobody has.
    ///
    /// Stamps are skipped for a session this journal does not own (a desktop pane, or one that
    /// ended). The observation tail is not session-scoped that way: `replay` serves every
    /// session the observer watches, and a desktop pane's readings are exactly what a waiting
    /// supervisor replays.
    ///
    /// Neither write moves `session_revision` or emits a machine event: an observation is not a
    /// session metadata change.
    pub(crate) fn record_journal(&self, batch: JournalBatch) -> Result<(), String> {
        let JournalBatch {
            stamps,
            observations,
        } = batch;
        if stamps.is_empty() && observations.is_empty() {
            return Ok(());
        }
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let mut changed = false;
        for (id, stamps) in &stamps {
            let Some(session) = candidate.sessions.get_mut(id) else {
                continue;
            };
            if session.agent_stamps.as_ref() != Some(stamps) {
                session.agent_stamps = Some(stamps.clone());
                changed = true;
            }
        }
        for (instance, observation) in &observations {
            changed |= candidate.observation_history.push(
                instance,
                observation,
                crate::agent_control::events::AGENT_EVENT_RETAIN,
            );
        }
        if !changed {
            return Ok(());
        }
        self.persist(&mut state, candidate)
    }

    /// Drops the durable observation tail, keeping the per-session stamps. Used by the durability
    /// test to prove the replay assertion depends on these rows; no production path deletes
    /// history, because a run that cannot resume it must not destroy it either.
    /// Returns whether any row was removed.
    #[cfg(test)]
    pub(crate) fn clear_observation_history(&self) -> Result<bool, String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        if candidate.observation_history.is_empty() {
            return Ok(false);
        }
        candidate.observation_history = ObservationHistory::default();
        self.persist(&mut state, candidate)?;
        Ok(true)
    }
    /// One screen read against the pending-input table: the card this read found is opened (or its
    /// revision refreshed), and every other open card of the same pane is settled as `gone`.
    ///
    /// One write for the whole read, so a poll cannot leave the pane half reconciled. A read that
    /// changes nothing writes nothing.
    pub(crate) fn reconcile_pending_inputs(
        &self,
        session_id: &str,
        found: Option<crate::agent_control::pending_input::NewPendingInput>,
        now: u64,
    ) -> Result<Option<crate::agent_control::pending_input::PendingInput>, String> {
        #[cfg(test)]
        self.probe("reconcile_pending_inputs");
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let before = state.as_ref().map_err(Clone::clone)?.clone();
        let mut candidate = before.clone();
        let found_id = found.as_ref().map(|input| input.prompt_id.clone());
        // Every other open card of this pane left the screen: the read that found a different card
        // (or no card at all) is the evidence, and leaving the row open would offer a phantom card.
        for (id, input) in candidate.pending_inputs.iter_mut() {
            if Some(id.as_str()) == found_id.as_deref() {
                continue;
            }
            if input.session_id != session_id || input.resolution.is_some() {
                continue;
            }
            if input.is_in_crash_window()
                || input.attempt.as_ref().is_some_and(|attempt| attempt.outcome.as_deref() == Some("outcomeUnknown"))
            {
                continue;
            }
            input.resolution = Some(crate::agent_control::pending_input::PendingInputResolution {
                at: now,
                outcome: "gone".into(),
                receipt: None,
            });
        }
        let recorded = match found {
            Some(request) => {
                let record = crate::agent_control::pending_input::adopt(
                    candidate.pending_inputs.get(&request.prompt_id).cloned(),
                    request,
                    now,
                )?;
                candidate
                    .pending_inputs
                    .insert(record.prompt_id.clone(), record.clone());
                Some(record)
            }
            None => None,
        };
        if candidate.pending_inputs == before.pending_inputs {
            return Ok(recorded);
        }
        candidate.pending_inputs.retain(|_, input| {
            input
                .resolution
                .as_ref()
                .is_none_or(|resolved| now.saturating_sub(resolved.at) < 7 * 86400)
        });
        self.persist(&mut state, candidate)?;
        Ok(recorded)
    }
    /// This journal's pending-input table, for tests and for the daemon's startup retry.
    pub(crate) fn pending_input(
        &self,
        prompt_id: &str,
    ) -> Result<Option<crate::agent_control::pending_input::PendingInput>, String> {
        #[cfg(test)]
        self.probe("pending_input");
        Ok(self
            .state
            .lock()
            .as_ref()
            .map_err(Clone::clone)?
            .pending_inputs
            .get(prompt_id)
            .cloned())
    }
    /// Every pending input still open, oldest first: what a restarted daemon owes.
    pub(crate) fn open_pending_inputs(
        &self,
    ) -> Result<Vec<crate::agent_control::pending_input::PendingInput>, String> {
        #[cfg(test)]
        self.probe("open_pending_inputs");
        let state = self.state.lock();
        let mut open: Vec<_> = state
            .as_ref()
            .map_err(Clone::clone)?
            .pending_inputs
            .values()
            .filter(|input| input.is_open())
            .cloned()
            .collect();
        open.sort_by_key(|input| (input.opened_at, input.prompt_id.clone()));
        Ok(open)
    }
    /// Read one pending input and rebuild it with `edit`, persisting only when `edit` reports a
    /// change. Read-modify-write under the same lock and file lock the rest of the journal uses,
    /// so two callers cannot interleave a claim and a receipt for the same `prompt_id`.
    ///
    /// The edit runs on a clone: a closure that refuses (invalid transition, nothing to retry)
    /// leaves the file and the in-memory store untouched.
    pub(crate) fn update_pending_input<T>(
        &self,
        prompt_id: &str,
        edit: impl FnOnce(Option<crate::agent_control::pending_input::PendingInput>) -> (Option<crate::agent_control::pending_input::PendingInput>, bool, T),
    ) -> Result<T, String> {
        #[cfg(test)]
        self.probe("update_pending_input");
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let current = state
            .as_ref()
            .map_err(Clone::clone)?
            .pending_inputs
            .get(prompt_id)
            .cloned();
        let (next, changed, result) = edit(current);
        if !changed {
            return Ok(result);
        }
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        match next {
            Some(record) => {
                candidate.pending_inputs.insert(prompt_id.to_string(), record);
            }
            None => {
                candidate.pending_inputs.remove(prompt_id);
            }
        }
        let now = now();
        // Bounded: answered cards are dropped after a week, so the journal cannot grow without
        // limit. A card still open is never dropped - the agent is blocked on it.
        candidate.pending_inputs.retain(|_, input| {
            input
                .resolution
                .as_ref()
                .is_none_or(|resolved| now.saturating_sub(resolved.at) < 7 * 86400)
        });
        self.persist(&mut state, candidate)?;
        Ok(result)
    }
    pub(crate) fn save_session(&self, session: MachineSession) -> Result<(), String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let mut merged = session;
        if let Some(current) = candidate.sessions.get(&merged.session.target.session_id) {
            if current.session.target != merged.session.target {
                return Err("STALE_EPOCH".into());
            }
            // Stamps are the observer's, never the caller's snapshot of the session.
            merged.agent_stamps = current.agent_stamps.clone();
            merged.session.title = current.session.title.clone();
            merged.session.cwd = current.session.cwd.clone();
            merged.session.agent_type = current.session.agent_type.clone();
            merged.session.provider_session = current.session.provider_session.clone();
        }
        candidate
            .sessions
            .insert(merged.session.target.session_id.clone(), merged);
        candidate.session_revision = candidate
            .session_revision
            .checked_add(1)
            .ok_or("CAPACITY_EXCEEDED")?;
        self.persist(&mut state, candidate)
    }
    /// Commit only owner-derived metadata, fenced against exit and owner replacement.
    pub(crate) fn update_session_metadata(
        &self,
        session: &super::machine_protocol::Session,
    ) -> Result<
        Option<(
            super::machine_protocol::Session,
            crate::scoped_contracts::Epoch,
        )>,
        String,
    > {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let current = candidate
            .sessions
            .get_mut(&session.target.session_id)
            .ok_or("SESSION_NOT_FOUND")?;
        if current.session.target != session.target {
            return Err("STALE_EPOCH".into());
        }
        if current.exit.is_some() || !current.session.running {
            return Err("SESSION_EXPIRED".into());
        }
        if current.session.cwd == session.cwd
            && current.session.title == session.title
            && current.session.agent_type == session.agent_type
            && current.session.provider_session == session.provider_session
        {
            return Ok(None);
        }
        current.session.cwd = session.cwd.clone();
        current.session.title = session.title.clone();
        current.session.agent_type = session.agent_type.clone();
        current.session.provider_session = session.provider_session.clone();
        let updated = current.session.clone();
        candidate.session_revision = candidate
            .session_revision
            .checked_add(1)
            .ok_or("CAPACITY_EXCEEDED")?;
        let revision = crate::scoped_contracts::Epoch(candidate.session_revision);
        self.persist(&mut state, candidate)?;
        Ok(Some((updated, revision)))
    }
    /// Preserve ambiguity without allowing a same-process retry to repeat Git.
    pub(crate) fn mark_unknown(&self, device: &str, request: &str) -> Result<(), String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let record = candidate
            .records
            .get_mut(&key(device, request))
            .ok_or("OPERATION_NOT_FOUND")?;
        record.status = 409;
        record.operation = Operation::OutcomeUnknown {
            request_id: request.into(),
        };
        self.persist(&mut state, candidate)
    }
    pub fn complete(
        &self,
        device_id: &str,
        request_id: &str,
        status: u16,
        outcome: OperationOutcome,
    ) -> Result<Record, String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut s = state.as_ref().map_err(Clone::clone)?.clone();
        let r = s
            .records
            .get_mut(&key(device_id, request_id))
            .ok_or("OPERATION_NOT_FOUND")?;
        if !matches!(
            r.operation,
            Operation::Pending { .. } | Operation::OutcomeUnknown { .. }
        ) {
            return Err("REQUEST_CONFLICT".into());
        }
        r.status = status;
        r.completed_at = Some(now());
        r.outcome_summary = Some(match &outcome {
            OperationOutcome::Error { error } => error.code.clone(),
            OperationOutcome::NoContent => "noContent".into(),
            OperationOutcome::Project { .. } => "project".into(),
            OperationOutcome::Worktree { .. } => "worktree".into(),
            OperationOutcome::Session { .. } => "session".into(),
        });
        r.operation = Operation::Completed {
            request_id: request_id.into(),
            outcome: Box::new(outcome),
        };
        let result = r.clone();
        self.persist(&mut state, s)?;
        Ok(result)
    }
}

#[cfg(test)]
impl MachineOperationJournal {
    fn probe(&self, phase: &str) {
        let probe = self.probe.read().clone();
        if let Some(probe) = probe {
            probe(phase);
        }
    }
}

#[cfg(test)]
#[path = "journal_contention_tests.rs"]
mod contention_tests;

#[cfg(test)]
mod agent_stamp_tests {
    use super::*;
    use crate::agent_control::{events::SessionStamps, AgentControlState};
    use crate::scoped_contracts::Epoch;

    fn stamps(revision: &str, since: i64, changed: i64) -> SessionStamps {
        SessionStamps {
            state: AgentControlState::Working,
            screen_revision: Some(revision.into()),
            state_since_ms: Some(since),
            screen_changed_at_ms: Some(changed),
            read: None,
        }
    }

    fn machine_session(id: &str) -> MachineSession {
        MachineSession {
            creator_device: "device".into(),
            session: super::super::machine_protocol::Session {
                title: None,
                agent_type: None,
                target: super::super::machine_protocol::RemoteTerminalTarget {
                    machine_id: "machine".into(),
                    daemon_epoch: Epoch(1),
                    session_id: id.into(),
                },
                workspace_id: "ws".into(),
                worktree: None,
                cwd: "/work".into(),
                cols: 80,
                rows: 24,
                running: true,
                provider_session: None,
                start_sequence: Epoch(0),
                end_sequence: Epoch(0),
                pid: None,
            },
            exit: None,
            agent_stamps: None,
        }
    }

    fn journal_with(dir: &tempfile::TempDir, ids: &[&str]) -> MachineOperationJournal {
        let journal = MachineOperationJournal::open(dir.path().join("machine-operations.v1.json"));
        for id in ids {
            journal.save_session(machine_session(id)).unwrap();
        }
        journal
    }

    #[test]
    fn stamps_round_trip_through_the_file_and_a_reopened_journal() {
        let dir = tempfile::tempdir().unwrap();
        let journal = journal_with(&dir, &["s1", "s2"]);
        assert!(journal.agent_stamps().unwrap().is_empty(), "nothing is persisted before a reading");
        let batch = BTreeMap::from([("s1".to_string(), stamps("r1", 100, 200))]);
        journal.record_agent_stamps(&batch).unwrap();

        let reopened = MachineOperationJournal::open(dir.path().join("machine-operations.v1.json"));
        let loaded = reopened.agent_stamps().unwrap();
        assert_eq!(loaded.len(), 1, "only a session with stamps appears: {loaded:?}");
        assert_eq!(loaded["s1"], stamps("r1", 100, 200));
    }

    #[test]
    fn recording_stamps_for_a_session_the_journal_does_not_own_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let journal = journal_with(&dir, &["s1"]);
        let revision = journal.session_revision().unwrap();
        let batch = BTreeMap::from([("desktop-pane".to_string(), stamps("r", 1, 2))]);
        journal.record_agent_stamps(&batch).unwrap();
        assert!(journal.agent_stamps().unwrap().is_empty());
        assert_eq!(journal.session_revision().unwrap(), revision, "stamps never move the session revision");
    }

    #[test]
    fn saving_a_session_snapshot_keeps_the_observers_stamps() {
        let dir = tempfile::tempdir().unwrap();
        let journal = journal_with(&dir, &["s1"]);
        journal
            .record_agent_stamps(&BTreeMap::from([("s1".to_string(), stamps("r1", 100, 200))]))
            .unwrap();
        // A later save carries the caller's snapshot of the session, which has no stamps.
        journal.save_session(machine_session("s1")).unwrap();
        assert_eq!(journal.agent_stamps().unwrap()["s1"], stamps("r1", 100, 200));
    }

    #[test]
    fn a_journal_written_before_stamps_existed_loads_with_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("machine-operations.v1.json");
        let journal = journal_with(&dir, &["s1"]);
        drop(journal);
        // The file as an older build wrote it: the same session record without the field.
        let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(value["sessions"]["s1"].get("agentStamps").is_none(), "no stamps are written until there are some");
        value["sessions"]["s1"].as_object_mut().unwrap().remove("agentStamps");
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();

        let legacy = MachineOperationJournal::open(path);
        assert!(legacy.session("s1").unwrap().is_some(), "the legacy journal loads");
        assert!(legacy.agent_stamps().unwrap().is_empty());
        legacy
            .record_agent_stamps(&BTreeMap::from([("s1".to_string(), stamps("r1", 5, 6))]))
            .unwrap();
        assert_eq!(legacy.agent_stamps().unwrap()["s1"], stamps("r1", 5, 6));
    }

    #[test]
    fn a_persisted_restart_keeps_the_old_times_on_an_unchanged_reading() {
        use crate::agent_control::{events::AgentEventBus, events::ObservationDraft, AgentStateSource, AgentTier};
        let dir = tempfile::tempdir().unwrap();
        let journal = journal_with(&dir, &["s1"]);
        journal
            .record_agent_stamps(&BTreeMap::from([("s1".to_string(), stamps("rev", 100, 200))]))
            .unwrap();
        drop(journal);

        // A new daemon run: a fresh bus seeded from the reopened journal, a clock far later.
        let reopened = MachineOperationJournal::open(dir.path().join("machine-operations.v1.json"));
        let bus = AgentEventBus::new(8);
        bus.seed_stamps(reopened.agent_stamps().unwrap());
        let reading = |revision: &str| ObservationDraft {
            session_id: "s1".into(),
            state: AgentControlState::Working,
            state_source: AgentStateSource::Provider,
            tier: AgentTier::Screen,
            prompt_id: None,
            kind: None,
            screen_revision: Some(revision.into()),
            workspace_id: None,
        };
        let unchanged = bus.publish(reading("rev"));
        assert_eq!(unchanged.state_since_ms, Some(100));
        assert_eq!(unchanged.screen_changed_at_ms, Some(200));
        let moved = bus.publish(reading("rev-2"));
        assert_eq!(moved.state_since_ms, Some(100));
        assert!(moved.screen_changed_at_ms.is_some_and(|at| at > 200), "{moved:?}");
    }

    /// The durability claim of T1, on the real file: publish through the bus with the daemon's
    /// writer attached, drop the bus, reopen the journal, and a same-epoch cursor replays the
    /// same rows. Deleting the durable rows must break that replay, so the test cannot pass on
    /// the pre-existing stamp path.
    #[test]
    fn observations_survive_a_restart_and_replay_for_a_same_epoch_cursor_only() {
        use crate::agent_control::events::{
            AgentEventBus, JournalUpdate, ObservationDraft, AGENT_EVENT_RETAIN,
        };
        use crate::agent_control::{AgentStateSource, AgentTier};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("machine-operations.v1.json");
        let draft = |state: AgentControlState, revision: &str| ObservationDraft {
            session_id: "s1".into(),
            state,
            state_source: AgentStateSource::Screen,
            tier: AgentTier::Screen,
            prompt_id: None,
            kind: None,
            screen_revision: Some(revision.into()),
            workspace_id: None,
        };

        // One daemon run: a bus whose published rows the journal's writer applies, exactly as
        // `install_agent_stamp_persistence` wires it (one queue, one writer).
        let journal = MachineOperationJournal::open(path.clone());
        let bus = AgentEventBus::new(AGENT_EVENT_RETAIN);
        let (queue, updates) = std::sync::mpsc::channel();
        let sink_queue = std::sync::Mutex::new(queue);
        bus.set_observation_sink(std::sync::Arc::new(move |instance, observation| {
            if let Ok(queue) = sink_queue.lock() {
                let _ = queue.send(JournalUpdate::Observed(
                    instance.to_string(),
                    observation.clone(),
                ));
            }
        }));
        let published = [
            bus.publish(draft(AgentControlState::Working, "rev-1")),
            bus.publish(draft(AgentControlState::Blocked, "rev-2")),
            bus.publish(draft(AgentControlState::Idle, "rev-3")),
        ];
        let epoch = bus.instance().to_string();
        let cursor = published[0].sequence;
        let expected: Vec<_> = published[1..].to_vec();
        match bus.replay(cursor) {
            crate::agent_control::events::AgentReplay::Events { events, .. } => {
                assert_eq!(
                    events, expected,
                    "the running bus is the only same-epoch replayer: adopt_history mints a fresh epoch"
                );
            }
            crate::agent_control::events::AgentReplay::Gap { .. } => {
                panic!("a same-epoch cursor inside the retained tail must replay, not gap")
            }
        }
        // The writer's own batch loop, run to completion by closing the queue.
        bus.clear_stamp_sink();
        let (batch, closed) = crate::agent_control::events::next_journal_batch(
            &updates,
            std::time::Duration::from_millis(1),
        )
        .expect("a batch");
        assert!(closed);
        journal.record_journal(batch).unwrap();
        drop(bus);
        drop(journal);

        let reopened = MachineOperationJournal::open(path.clone());
        let stored = reopened.observation_history().unwrap();
        assert_eq!(stored.instance.as_deref(), Some(epoch.as_str()), "the epoch is durable");
        assert_eq!(stored.retained.len(), 3, "every row is durable, not only the newest");
        let restarted = AgentEventBus::new(AGENT_EVENT_RETAIN);
        assert!(restarted.adopt_history(stored), "the restarted bus adopts the stored history");
        assert_ne!(restarted.instance(), epoch, "the restarted bus has a fresh cursor epoch");
        match restarted.replay_in_instance(cursor, Some(&epoch)) {
            crate::agent_control::events::AgentReplay::Gap { snapshot, .. } => {
                assert_eq!(snapshot, vec![published[2].clone()]);
            }
            crate::agent_control::events::AgentReplay::Events { events, .. } => {
                panic!("a pre-restart cursor must gap, got {events:?}")
            }
        }
        assert_eq!(restarted.snapshot(), vec![published[2].clone()]);

        // A cursor from another epoch is a gap plus that snapshot, never a wrong replay.
        match restarted.replay(published[2].sequence + 1) {
            crate::agent_control::events::AgentReplay::Gap { snapshot, .. } => {
                assert_eq!(snapshot, vec![published[2].clone()]);
            }
            crate::agent_control::events::AgentReplay::Events { events, .. } => {
                panic!("a cross-epoch cursor must gap: {events:?}")
            }
        }

        let restarted_epoch = restarted.instance().to_owned();
        let overflow: Vec<_> = (0..AGENT_EVENT_RETAIN + 2)
            .map(|n| {
                let observation = restarted.publish(draft(
                    AgentControlState::Working,
                    &format!("overflow-{n}"),
                ));
                (restarted_epoch.clone(), observation)
            })
            .collect();
        let retained_cursor = overflow[overflow.len() - 2].1.sequence;
        let newest = overflow[overflow.len() - 1].1.clone();
        reopened.record_journal(crate::agent_control::events::JournalBatch {
            stamps: BTreeMap::new(),
            observations: overflow,
        }).unwrap();
        let evicted = MachineOperationJournal::open(path.clone())
            .observation_history().unwrap();
        assert!(evicted.evicted_through > cursor, "the file records a genuinely evicted head");
        let trimmed = AgentEventBus::new(AGENT_EVENT_RETAIN);
        assert!(trimmed.adopt_history(evicted));
        assert!(matches!(
            trimmed.replay(cursor),
            crate::agent_control::events::AgentReplay::Gap { .. }
        ), "a cursor behind the evicted head must gap");
        match restarted.replay_in_instance(retained_cursor, Some(&restarted_epoch)) {
            crate::agent_control::events::AgentReplay::Events { events, .. } => {
                assert_eq!(
                    events, vec![newest.clone()],
                    "a retained row replays on the epoch that issued it, eviction notwithstanding"
                );
            }
            crate::agent_control::events::AgentReplay::Gap { .. } => {
                panic!("a retained cursor of its own epoch must not gap while the row is retained")
            }
        }
        match trimmed.replay_in_instance(retained_cursor, Some(&restarted_epoch)) {
            crate::agent_control::events::AgentReplay::Gap { snapshot, .. } => {
                assert_eq!(
                    snapshot, vec![newest],
                    "a reloaded tail survives as the gap snapshot, never as a previous-epoch replay"
                );
            }
            crate::agent_control::events::AgentReplay::Events { events, .. } => {
                panic!("a previous-epoch cursor must gap even while its row is retained: {events:?}")
            }
        }

        // Clearing the entire history also starts a new epoch, independently of eviction.
        assert!(reopened.clear_observation_history().unwrap());
        let emptied = MachineOperationJournal::open(path);
        let after_delete = emptied.observation_history().unwrap();
        assert!(after_delete.is_empty(), "the rows are gone from the file");
        let no_history = AgentEventBus::new(AGENT_EVENT_RETAIN);
        assert!(!no_history.adopt_history(after_delete), "nothing left to resume");
        assert_ne!(no_history.instance(), epoch, "without the rows the epoch is a new one");
        match no_history.replay(cursor) {
            crate::agent_control::events::AgentReplay::Gap { .. } => {}
            crate::agent_control::events::AgentReplay::Events { events, .. } => {
                panic!("the replay must fail once the durable rows are deleted, got {events:?}")
            }
        }
    }
}
