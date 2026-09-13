//! One daemon-owned durable journal. All methods are blocking; callers use run_blocking.
use super::machine_protocol::{Operation, OperationOutcome};
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
}

/// Durable ownership survives output-hub cleanup and journal result compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSession {
    pub creator_device: String,
    pub session: super::machine_protocol::Session,
    pub exit: Option<super::machine_protocol::ExitMetadata>,
}
pub struct MachineOperationJournal {
    path: PathBuf,
    state: parking_lot::Mutex<Result<Store, String>>,
    #[cfg(test)]
    probe: parking_lot::RwLock<Option<std::sync::Arc<dyn Fn(&str) + Send + Sync>>>,
}
pub enum Begin {
    New,
    Existing(Record),
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
        let pending = self.reconcile(&receipt.device_id, &receipt.request_id)?.ok_or("MACHINE_SERVICE_UNAVAILABLE")?;
        if pending.digest != receipt.digest || pending.kind != receipt.kind || pending.resource != receipt.resource {
            return Err("MACHINE_SERVICE_UNAVAILABLE".into());
        }
        if matches!(pending.operation, Operation::Pending { .. } | Operation::OutcomeUnknown { .. }) {
            let Operation::Completed { outcome, .. } = &receipt.operation else { return Err("MACHINE_SERVICE_UNAVAILABLE".into()); };
            self.complete(&receipt.device_id, &receipt.request_id, receipt.status, outcome.clone())?;
        }
        Ok(())
    }
    pub(crate) fn catalog_receipt(&self, device: &str, request: &str, status: u16, outcome: OperationOutcome) -> Result<Record, String> {
        let mut record = self.reconcile(device, request)?.ok_or("OPERATION_NOT_FOUND")?;
        if !matches!(record.operation, Operation::Pending { .. }) { return Err("REQUEST_CONFLICT".into()); }
        record.status = status;
        record.operation = Operation::Completed { request_id: request.into(), outcome };
        Ok(record)
    }
    pub(crate) fn open(path: PathBuf) -> Self {
        let state = match std::fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store {
                version: 1,
                records: BTreeMap::new(),
                sessions: BTreeMap::new(),
                session_revision: 0,
            }),
            Err(_) => Err("MACHINE_SERVICE_UNAVAILABLE".into()),
            Ok(bytes) => serde_json::from_slice::<Store>(&bytes)
                .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".into())
                .and_then(|mut s| {
                    if s.version != 1 {
                        return Err("MACHINE_SERVICE_UNAVAILABLE".into());
                    }
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
        let lock = options.open(self.path.with_extension("tx.lock")).map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        lock.lock().map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let mut current: Store = serde_json::from_slice(&bytes).map_err(|_| "MACHINE_SERVICE_UNAVAILABLE")?;
                if current.version != 1 { return Err("MACHINE_SERVICE_UNAVAILABLE".into()); }
                let previous = state.as_ref().map_err(Clone::clone)?;
                for (key, record) in &mut current.records {
                    if matches!(record.operation, Operation::Pending { .. })
                        && previous.records.get(key).is_some_and(|prior| matches!(prior.operation, Operation::OutcomeUnknown { .. })) {
                        record.operation = Operation::OutcomeUnknown { request_id: record.request_id.clone() };
                    }
                }
                *state = Ok(current);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
            Err(_) => return Err("MACHINE_SERVICE_UNAVAILABLE".into()),
        }
        Ok(lock)
    }

    fn persist(&self, state: &mut Result<Store, String>, candidate: Store) -> Result<(), String> {
        #[cfg(test)]
        self.probe("persist");
        let result = super::auth::write_private_json(&self.path, &candidate)
            .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string())
            .and_then(|_| {
                #[cfg(unix)]
                std::fs::File::open(self.path.parent().expect("journal parent"))
                    .and_then(|f| f.sync_all())
                    .map_err(|_| "MACHINE_SERVICE_UNAVAILABLE".to_string())?;
                Ok(())
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
            return Ok(Begin::Existing(r.clone()));
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
        Ok(state.as_ref().map_err(Clone::clone)?.records
            .get(&key(device_id, request_id))
            .cloned())
    }
    pub(crate) fn sessions(&self) -> Result<Vec<MachineSession>, String> {
        #[cfg(test)]
        self.probe("sessions");
        Ok(self.state.lock().as_ref().map_err(Clone::clone)?.sessions.values().cloned().collect())
    }
    pub(crate) fn session_revision(&self) -> Result<crate::scoped_contracts::Epoch, String> {
        Ok(crate::scoped_contracts::Epoch(self.state.lock().as_ref().map_err(Clone::clone)?.session_revision))
    }
    /// Includes unacknowledged spawns, fencing mirror admission before metadata publication.
    pub(crate) fn owns_session(&self, id: &str) -> bool {
        let state = self.state.lock();
        let Ok(store) = state.as_ref() else { return true; };
        store.sessions.contains_key(id) || store.records.values().any(|record| {
            record.kind == "createSession" && serde_json::from_str::<super::machine_protocol::RemoteTerminalTarget>(&record.resource)
                .is_ok_and(|target| target.session_id == id)
        })
    }
    /// Ownership and the original create receipt share one durable replacement.
    pub(crate) fn commit_spawn(&self, device: &str, request: &str, session: MachineSession) -> Result<(), String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let record = candidate.records.get_mut(&key(device, request)).ok_or("OPERATION_NOT_FOUND")?;
        if record.kind != "createSession" || record.resource != serde_json::to_string(&session.session.target).expect("target")
            || !matches!(record.operation, Operation::Pending { .. } | Operation::OutcomeUnknown { .. }) {
            return Err("REQUEST_CONFLICT".into());
        }
        record.status = 201;
        record.completed_at = Some(now());
        record.outcome_summary = Some("session".into());
        record.operation = Operation::Completed { request_id: request.into(), outcome: OperationOutcome::Session { session: session.session.clone() } };
        candidate.sessions.insert(session.session.target.session_id.clone(), session);
        candidate.session_revision = candidate.session_revision.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
        self.persist(&mut state, candidate)
    }
    pub(crate) fn session(&self, id: &str) -> Result<Option<MachineSession>, String> {
        Ok(self.state.lock().as_ref().map_err(Clone::clone)?.sessions.get(id).cloned())
    }
    pub(crate) fn save_session(&self, session: MachineSession) -> Result<(), String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let mut merged = session;
        if let Some(current) = candidate.sessions.get(&merged.session.target.session_id) {
            if current.session.target != merged.session.target { return Err("STALE_EPOCH".into()); }
            merged.session.title = current.session.title.clone();
            merged.session.cwd = current.session.cwd.clone();
            merged.session.agent_type = current.session.agent_type.clone();
            merged.session.provider_session = current.session.provider_session.clone();
        }
        candidate.sessions.insert(merged.session.target.session_id.clone(), merged);
        candidate.session_revision = candidate.session_revision.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
        self.persist(&mut state, candidate)
    }
    /// Commit only owner-derived metadata, fenced against exit and owner replacement.
    pub(crate) fn update_session_metadata(&self, session: &super::machine_protocol::Session)
        -> Result<Option<(super::machine_protocol::Session, crate::scoped_contracts::Epoch)>, String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let current = candidate.sessions.get_mut(&session.target.session_id).ok_or("SESSION_NOT_FOUND")?;
        if current.session.target != session.target { return Err("STALE_EPOCH".into()); }
        if current.exit.is_some() || !current.session.running { return Err("SESSION_EXPIRED".into()); }
        if current.session.cwd == session.cwd && current.session.title == session.title
            && current.session.agent_type == session.agent_type && current.session.provider_session == session.provider_session {
            return Ok(None);
        }
        current.session.cwd = session.cwd.clone();
        current.session.title = session.title.clone();
        current.session.agent_type = session.agent_type.clone();
        current.session.provider_session = session.provider_session.clone();
        let updated = current.session.clone();
        candidate.session_revision = candidate.session_revision.checked_add(1).ok_or("CAPACITY_EXCEEDED")?;
        let revision = crate::scoped_contracts::Epoch(candidate.session_revision);
        self.persist(&mut state, candidate)?;
        Ok(Some((updated, revision)))
    }
    /// Preserve ambiguity without allowing a same-process retry to repeat Git.
    pub(crate) fn mark_unknown(&self, device: &str, request: &str) -> Result<(), String> {
        let mut state = self.state.lock();
        let _file_lock = self.refresh_locked(&mut state)?;
        let mut candidate = state.as_ref().map_err(Clone::clone)?.clone();
        let record = candidate.records.get_mut(&key(device, request)).ok_or("OPERATION_NOT_FOUND")?;
        record.status = 409;
        record.operation = Operation::OutcomeUnknown { request_id: request.into() };
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
            outcome,
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
        if let Some(probe) = probe { probe(phase); }
    }
}

#[cfg(test)]
#[path = "journal_contention_tests.rs"]
mod contention_tests;
