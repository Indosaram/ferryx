//! Private QA barrier channel behind the `local-split-qa` Cargo feature.
//!
//! Task 3 (local-pane-liveness-root-remediation) product side of the runner
//! contract in `scripts/lib/qa-scenarios/common-harness.mjs`:
//!
//! - env: `FERRYX_QA_BARRIER_DIR` (task-owned control dir) and
//!   `FERRYX_QA_RUN_ID` (run nonce, echoed as `runId` by every emission).
//! - files: `<name>.arm.json` (runner, pre-launch) -> `<name>.armed-ack.json`
//!   (product, startup scan) -> `<name>.held.json` (product, at stage entry) ->
//!   `<name>.release.json` (runner) -> `<name>.receipt.jsonl` (product, one
//!   JSON object per settlement line).
//! - every arm/ack/held/release/receipt binds `runId` + `operationId` and the
//!   producer identity (`producer` component id + `producerPid`).
//!
//! The diagnostic-classifier headless harness drives the REAL Task 2 write
//! stage (`send_native_terminal_input_with_stage_logging`) and the REAL
//! presentation coordinator producer paths in process without windows, using
//! the real collector/classifier (`classify_pane_liveness`). Verdicts are
//! never fabricated: recovery receipts embed the fresh snapshot plus explicit
//! stage-progress evidence, and an `Unknown` verdict always carries
//! `evidenceMissing: true` with the missing field list. Actual OS/GPU
//! presentation proof stays native: headless runs record
//! `nativeEvidence: "deferred-to-task-10"`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{json, Value};
use tauri::Manager;

use crate::ipc::debug::{classify_pane_liveness, PaneLivenessSnapshot, PaneLivenessVerdict};
use crate::native_terminal::surface_host::NativeTerminalSurfaceHostState;

pub const PRODUCER_ID: &str = "ipc-qa-barrier";
pub const WRITE_BARRIER: &str = "backend-write";
pub const PRESENTATION_BARRIER: &str = "presentation";
pub const ATTACH_HANDSHAKE_BARRIER: &str = "attach-handshake";
pub const PREDECESSOR_EXPORT_BARRIER: &str = "predecessor-export";
pub const SUCCESSOR_ADOPT_BARRIER: &str = "successor-adopt";
pub const COMMIT_BARRIER: &str = "commit";
pub const ABORT_BARRIER: &str = "abort";
pub const HELD_RPC_BARRIER: &str = "held-rpc";

/// Release-watch cadence of the feature-gated watcher task. Never a hot path:
/// the channel only exists when the runner installed a private barrier dir.
const RELEASE_POLL_MS: u64 = 25;
/// The classifier treats a write pending for >250ms as `BlockedInIpcWrite`.
/// The held receipt is only written once the real pending age passed that
/// threshold, so the verdict is measured evidence, not an artifact of
/// measurement timing.
const WRITE_PENDING_THRESHOLD_GRACE_MS: u64 = 300;
const DEFAULT_DEADLINE_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseOutcome {
    Released,
    DeadlineExceeded,
}

impl ReleaseOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseOutcome::Released => "released",
            ReleaseOutcome::DeadlineExceeded => "deadline-exceeded",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ArmSpec {
    pub(crate) name: String,
    #[serde(rename = "runId")]
    pub(crate) run_id: String,
    #[serde(rename = "operationId")]
    pub(crate) operation_id: String,
    #[serde(rename = "deadlineMs", default = "default_deadline_ms")]
    pub(crate) deadline_ms: u64,
    #[serde(rename = "targetRole", default)]
    pub(crate) target_role: Option<String>,
    #[serde(rename = "targetBackendSessionId", default)]
    pub(crate) target_backend_session_id: Option<String>,
}

fn default_deadline_ms() -> u64 {
    DEFAULT_DEADLINE_MS
}

#[derive(Debug, Clone)]
pub struct QaBarrierHeldEvent {
    pub name: String,
    pub session_id: String,
    pub stage: String,
    pub payload: Value,
}

#[derive(Debug, Clone)]
pub struct QaBarrierReceiptEvent {
    pub name: String,
    pub line_index: usize,
    pub payload: Value,
}

pub struct QaBarrierChannel {
    dir: PathBuf,
    run_id: String,
    role: Option<String>,
    pid: u32,
    arms: Mutex<HashMap<String, ArmSpec>>,
    /// Line counters per receipt file so appends stay one-JSON-per-line.
    receipt_lines: Mutex<HashMap<String, usize>>,
    held_tx: tokio::sync::broadcast::Sender<QaBarrierHeldEvent>,
    receipt_tx: tokio::sync::broadcast::Sender<QaBarrierReceiptEvent>,
    held_rpc_claims: Mutex<Option<String>>,
    settled_rpc_ops: Mutex<HashSet<String>>,
    pending_workers: Mutex<Vec<tokio::task::JoinHandle<Result<(), String>>>>,
    sealed: Mutex<bool>,
}

static ACTIVE: RwLock<Option<Arc<QaBarrierChannel>>> = RwLock::new(None);

/// The producer-side channel, present only when the QA harness installed it
/// from the runner env. Default/release builds never reach any caller of this.
pub fn active_channel() -> Option<Arc<QaBarrierChannel>> {
    ACTIVE.read().ok().and_then(|guard| guard.clone())
}

pub(crate) fn install(channel: QaBarrierChannel) {
    if let Ok(mut guard) = ACTIVE.write() {
        *guard = Some(Arc::new(channel));
    }
}

pub(crate) fn deactivate() {
    if let Ok(mut guard) = ACTIVE.write() {
        *guard = None;
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn write_json_atomic(dir: &std::path::Path, file: &str, value: &Value) -> std::io::Result<()> {
    let tmp = dir.join(format!(".{file}.tmp-{}", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec_pretty(value).unwrap_or_default())?;
    std::fs::rename(tmp, dir.join(file))
}

impl QaBarrierChannel {
    pub fn new(dir: PathBuf, run_id: String) -> Self {
        Self::new_with_role(dir, run_id, None)
    }

    pub fn new_with_role(dir: PathBuf, run_id: String, role: Option<String>) -> Self {
        let (held_tx, _) = tokio::sync::broadcast::channel(64);
        let (receipt_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            dir,
            run_id,
            role,
            pid: std::process::id(),
            arms: Mutex::new(HashMap::new()),
            receipt_lines: Mutex::new(HashMap::new()),
            held_tx,
            receipt_tx,
            held_rpc_claims: Mutex::new(None),
            settled_rpc_ops: Mutex::new(HashSet::new()),
            pending_workers: Mutex::new(Vec::new()),
            sealed: Mutex::new(false),
        }
    }

    /// Runner contract: the channel only exists through the private inherited
    /// env. Any other invocation (production/default runtime) is refused.
    pub fn from_env() -> Result<Self, String> {
        let role = std::env::var("FERRYX_QA_ROLE").ok().filter(|r| !r.trim().is_empty());
        Self::from_env_with_role(role)
    }

    pub fn from_env_with_role(role: Option<String>) -> Result<Self, String> {
        let dir = std::env::var("FERRYX_QA_BARRIER_DIR")
            .map_err(|_| "FERRYX_QA_BARRIER_DIR is not set".to_string())?;
        let run_id = std::env::var("FERRYX_QA_RUN_ID")
            .map_err(|_| "FERRYX_QA_RUN_ID is not set".to_string())?;
        if dir.trim().is_empty() || run_id.trim().is_empty() {
            return Err("FERRYX_QA_BARRIER_DIR/FERRYX_QA_RUN_ID must be non-empty".into());
        }
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(format!("barrier dir does not exist: {}", dir.display()));
        }
        Ok(Self::new_with_role(dir, run_id, role))
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// Checks if an armed barrier targets a specific backend session ID.
    /// Explicitly rejects None => true: missing targetBackendSessionId does NOT match any pane!
    pub fn matches_target_session(&self, barrier_name: &str, session_id: &str) -> bool {
        match self
            .arms
            .lock()
            .ok()
            .and_then(|a| a.get(barrier_name).and_then(|s| s.target_backend_session_id.clone()))
        {
            Some(ref target) => target == session_id,
            None => false,
        }
    }

    /// Emits a bounded `<name>.bound-ack.json` receipt before attachment release,
    /// binding the exact backendSessionId and operationId to the armed barrier.
    pub fn write_bound_ack(
        &self,
        name: &str,
        session_id: &str,
        operation_id: &str,
    ) -> Result<(), String> {
        let ack = json!({
            "name": name,
            "runId": self.run_id,
            "operationId": operation_id,
            "producer": PRODUCER_ID,
            "producerPid": self.pid,
            "targetBackendSessionId": session_id,
            "boundAtMs": now_ms(),
        });
        let filename = format!("{name}.bound-ack.json");
        write_json_atomic(&self.dir, &filename, &ack).map_err(|e| e.to_string())
    }

    pub fn target_backend_session_id_for(&self, name: &str) -> Option<String> {
        self.arms
            .lock()
            .ok()?
            .get(name)
            .and_then(|s| s.target_backend_session_id.clone())
    }

    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }

    pub fn producer_pid(&self) -> u32 {
        self.pid
    }

    /// Per-barrier scoped operation ownership: returns the operationId bound
    /// specifically to this barrier's arm.
    pub fn operation_id_for(&self, name: &str) -> Option<String> {
        self.arms.lock().ok()?.get(name).map(|s| s.operation_id.clone())
    }

    /// Global fallback operation nonce across any armed barrier.
    pub fn operation_id(&self) -> Option<String> {
        self.arms
            .lock()
            .ok()
            .and_then(|arms| arms.values().next().map(|s| s.operation_id.clone()))
    }

    /// Startup registration: scan pre-existing arm files (the runner arms
    /// BEFORE launch) and ack each one whose run nonce matches. A mismatched
    /// nonce arm is rejected (never acked, never holdable) so stale/replayed
    /// controls from another run cannot be honored.
    pub fn scan_and_ack_arms(&self) -> (Vec<String>, Vec<String>) {
        let mut acked = Vec::new();
        let mut rejected = Vec::new();
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return (acked, rejected);
        };
        let mut arm_files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.ends_with(".arm.json"))
            })
            .collect();
        arm_files.sort();
        for path in arm_files {
            let name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .map(|n| n.trim_end_matches(".arm").to_string())
                .unwrap_or_default();
            if path.is_symlink() {
                rejected.push(format!("{name}:symlink-disallowed"));
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                rejected.push(format!("{name}:unreadable"));
                continue;
            };
            let Ok(spec) = serde_json::from_str::<ArmSpec>(&text) else {
                rejected.push(format!("{name}:unparseable"));
                continue;
            };
            if spec.name != name {
                rejected.push(format!("{name}:name-mismatch"));
                continue;
            }
            if spec.run_id != self.run_id {
                rejected.push(format!("{}:run-nonce-mismatch", spec.name));
                continue;
            }
            if spec.operation_id.trim().is_empty() {
                rejected.push(format!("{name}:empty-operation-id"));
                continue;
            }
            let effective_target_role = spec.target_role.as_deref().or_else(|| {
                match name.as_str() {
                    PREDECESSOR_EXPORT_BARRIER => Some("predecessor"),
                    SUCCESSOR_ADOPT_BARRIER => Some("successor"),
                    _ => None,
                }
            });
            if let Some(target) = effective_target_role {
                if let Some(ref our_role) = self.role {
                    if target != our_role {
                        // Arm is explicitly targeted at another process role (e.g. successor ignoring predecessor-export)
                        continue;
                    }
                }
            }
            let mut ack = json!({
                "name": spec.name,
                "runId": spec.run_id,
                "operationId": spec.operation_id,
                "producer": PRODUCER_ID,
                "producerPid": self.pid,
                "registeredAtMs": now_ms(),
            });
            if let Some(ref role) = self.role {
                if let Value::Object(ref mut map) = ack {
                    map.insert("role".to_string(), json!(role));
                }
            }
            if let Some(ref sid) = spec.target_backend_session_id {
                if let Value::Object(ref mut map) = ack {
                    map.insert("targetBackendSessionId".to_string(), json!(sid));
                }
            }
            let ack_role_filename = if let Some(ref role) = self.role {
                format!("{}.armed-ack.{role}.json", spec.name)
            } else {
                format!("{}.armed-ack.{}.json", spec.name, self.pid)
            };
            let _ = write_json_atomic(&self.dir, &ack_role_filename, &ack);

            let canonical_ack = self.dir.join(format!("{}.armed-ack.json", spec.name));
            let should_write_canonical = if !canonical_ack.exists() {
                true
            } else if let Ok(existing_text) = std::fs::read_to_string(&canonical_ack) {
                if let Ok(existing_json) = serde_json::from_str::<Value>(&existing_text) {
                    existing_json.get("role").and_then(|r| r.as_str()) == self.role.as_deref()
                        || existing_json.get("producerPid").and_then(|p| p.as_u64()) == Some(self.pid as u64)
                } else {
                    true
                }
            } else {
                false
            };
            if should_write_canonical {
                let _ = write_json_atomic(&self.dir, &format!("{}.armed-ack.json", spec.name), &ack);
            }

            if let Ok(mut arms) = self.arms.lock() {
                arms.insert(spec.name.clone(), spec);
            }
            acked.push(name);
        }
        (acked, rejected)
    }

    pub fn spec(&self, name: &str) -> Option<ArmSpec> {
        self.arms.lock().ok()?.get(name).cloned()
    }

    pub fn is_armed(&self, name: &str) -> bool {
        self.arms.lock().ok().map_or(false, |m| m.contains_key(name))
    }

    pub fn try_claim(&self, barrier_name: &str, session_id: &str, operation_id: &str) -> bool {
        let mut claims = self.held_rpc_claims.lock().unwrap();
        let key = format!("{barrier_name}:{session_id}:{operation_id}");
        if claims.as_deref() == Some(&key) {
            return false;
        }
        *claims = Some(key);
        true
    }

    pub fn release_claim(&self, barrier_name: &str, session_id: &str, operation_id: &str) -> bool {
        let mut claims = self.held_rpc_claims.lock().unwrap();
        let key = format!("{barrier_name}:{session_id}:{operation_id}");
        if claims.as_deref() == Some(&key) {
            *claims = None;
            true
        } else {
            false
        }
    }

    pub fn has_claim(&self, barrier_name: &str, session_id: &str, operation_id: &str) -> bool {
        let claims = self.held_rpc_claims.lock().unwrap();
        let key = format!("{barrier_name}:{session_id}:{operation_id}");
        claims.as_deref() == Some(&key)
    }

    pub fn flush_receipts(&self, name: &str) {
        let path = self.dir.join(format!("{name}.receipt.jsonl"));
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) {
            let _ = file.sync_data();
        }
    }

    pub fn try_claim_held_rpc(&self, operation_id: &str) -> bool {
        let mut claims = self.held_rpc_claims.lock().unwrap();
        let settled = self.settled_rpc_ops.lock().unwrap();
        if settled.contains(operation_id) {
            return false;
        }
        if let Some(ref current) = *claims {
            if current == operation_id {
                return false;
            }
        }
        *claims = Some(operation_id.to_string());
        true
    }

    pub fn release_held_rpc_claim(&self, operation_id: &str, settled: bool) {
        let mut claims = self.held_rpc_claims.lock().unwrap();
        if claims.as_deref() == Some(operation_id) {
            *claims = None;
        }
        if settled {
            self.settled_rpc_ops.lock().unwrap().insert(operation_id.to_string());
        }
    }

    pub fn schedule_cancellation_receipt(
        self: &Arc<Self>,
        name: &str,
        operation_id: &str,
        settlement: Value,
    ) {
        let mut lock = self.pending_workers.lock().unwrap();
        if *self.sealed.lock().unwrap() {
            drop(lock);
            let _ = self.append_receipt_checked(name, operation_id, settlement);
            return;
        }
        let ch = Arc::clone(self);
        let name = name.to_string();
        let op_id = operation_id.to_string();
        let handle = tokio::task::spawn_blocking(move || {
            ch.append_receipt_checked(&name, &op_id, settlement)
        });
        lock.push(handle);
    }

    /// Seal and drain all owned receipt workers, propagating any write or join
    /// failures into Result::Err instead of silently discarding errors.
    /// Drains repeatedly until the queue is completely empty even if live producers
    /// schedule workers concurrently during drop.
    pub async fn drain_and_verify_workers(&self) -> Result<(), String> {
        // Seal the queue so no further background workers can be scheduled
        *self.sealed.lock().unwrap() = true;

        loop {
            let handles = {
                let mut lock = self.pending_workers.lock().unwrap();
                if lock.is_empty() {
                    break;
                }
                std::mem::take(&mut *lock)
            };
            for handle in handles {
                match handle.await {
                    Ok(Ok(())) => {}
                    Ok(Err(write_err)) => {
                        return Err(format!("receipt append failed: {write_err}"));
                    }
                    Err(join_err) => {
                        return Err(format!("receipt worker panicked or failed: {join_err}"));
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn await_pending_workers(&self) {
        let _ = self.drain_and_verify_workers().await;
    }

    pub async fn hold_barrier(
        &self,
        name: &str,
        session_id: &str,
        stage: &str,
        extra: Value,
    ) -> Option<ReleaseOutcome> {
        let spec = self.spec(name)?;
        self.write_held(&spec, session_id, stage, extra);
        Some(self.wait_for_release(&spec).await)
    }

    /// Bounded, feature-gated watcher: resolves when the runner writes a
    /// release control whose name/run/operation identity matches this arm.
    /// Mismatched (stale/replayed/wrong-operation) controls are ignored; the
    /// deadline from the arm file bounds the wait so a lost release cancels
    /// the hold instead of parking the producer forever.
    pub async fn wait_for_release(&self, spec: &ArmSpec) -> ReleaseOutcome {
        let path = self.dir.join(format!("{}.release.json", spec.name));
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(spec.deadline_ms);
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(RELEASE_POLL_MS));
        loop {
            if let Ok(text) = tokio::fs::read_to_string(&path).await {
                if let Ok(control) = serde_json::from_str::<Value>(&text) {
                    let matches = control.get("name").and_then(Value::as_str) == Some(&spec.name)
                        && control.get("runId").and_then(Value::as_str) == Some(&spec.run_id)
                        && control.get("operationId").and_then(Value::as_str)
                            == Some(&spec.operation_id);
                    if matches {
                        return ReleaseOutcome::Released;
                    }
                    // A control with the wrong identity is rejected on record;
                    // the hold keeps waiting for the correlated release.
                    let _ = write_json_atomic(
                        &self.dir,
                        &format!("{}.rejected-control.json", spec.name),
                        &json!({
                            "runId": self.run_id,
                            "operationId": spec.operation_id,
                            "producer": PRODUCER_ID,
                            "producerPid": self.pid,
                            "rejectedAtMs": now_ms(),
                            "reason": "run-or-operation-identity-mismatch",
                        }),
                    );
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return ReleaseOutcome::DeadlineExceeded;
            }
            ticker.tick().await;
        }
    }

    fn correlation(&self, spec: &ArmSpec) -> Value {
        json!({
            "runId": self.run_id,
            "operationId": spec.operation_id,
            "producer": PRODUCER_ID,
            "producerPid": self.pid,
        })
    }

    pub fn write_held(
        &self,
        spec: &ArmSpec,
        session_id: &str,
        stage: &str,
        extra: Value,
    ) {
        let mut payload = self.correlation(spec);
        payload["name"] = json!(spec.name);
        payload["sessionId"] = json!(session_id);
        payload["stage"] = json!(stage);
        payload["heldAtMs"] = json!(now_ms());
        if let (Value::Object(base), Value::Object(add)) = (&mut payload, extra) {
            for (k, v) in add {
                base.insert(k, v);
            }
        }
        let _ = write_json_atomic(&self.dir, &format!("{}.held.json", spec.name), &payload);
        let _ = self.held_tx.send(QaBarrierHeldEvent {
            name: spec.name.clone(),
            session_id: session_id.to_string(),
            stage: stage.to_string(),
            payload,
        });
    }

    /// Append one settlement line to `<name>.receipt.jsonl`, carrying the run
    /// and operation nonces plus producer identity.
    pub fn append_receipt_checked(
        &self,
        name: &str,
        operation_id: &str,
        settlement: Value,
    ) -> Result<(), String> {
        let mut payload = json!({
            "runId": self.run_id,
            "operationId": operation_id,
            "producer": PRODUCER_ID,
            "producerPid": self.pid,
            "settledAtMs": now_ms(),
        });
        if let Some(ref role) = self.role {
            if let Value::Object(ref mut map) = payload {
                map.insert("role".to_string(), json!(role));
            }
        }
        if let (Value::Object(base), Value::Object(add)) = (&mut payload, settlement) {
            for (k, v) in add {
                base.insert(k, v);
            }
        }
        let line = format!("{}\n", payload.to_string());
        let path = self.dir.join(format!("{name}.receipt.jsonl"));
        let mut count = self
            .receipt_lines
            .lock()
            .ok()
            .and_then(|m| m.get(name).copied())
            .unwrap_or(0);
        // Re-read the existing line count so appends after a channel re-scan
        // still produce well-formed JSONL.
        if count == 0 {
            if let Ok(existing) = std::fs::read_to_string(&path) {
                count = existing.lines().filter(|l| !l.trim().is_empty()).count();
            }
        }
        use std::io::Write as _;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("open receipt file: {e}"))?;
        file.write_all(line.as_bytes())
            .map_err(|e| format!("write receipt: {e}"))?;
        file.sync_data()
            .map_err(|e| format!("sync receipt: {e}"))?;
        let next_count = count + 1;
        if let Ok(mut lines) = self.receipt_lines.lock() {
            lines.insert(name.to_string(), next_count);
        }
        let _ = self.receipt_tx.send(QaBarrierReceiptEvent {
            name: name.to_string(),
            line_index: next_count,
            payload,
        });
        Ok(())
    }

    pub fn append_receipt(&self, name: &str, operation_id: &str, settlement: Value) {
        let _ = self.append_receipt_checked(name, operation_id, settlement);
    }

    pub fn subscribe_held(&self) -> tokio::sync::broadcast::Receiver<QaBarrierHeldEvent> {
        self.held_tx.subscribe()
    }

    pub fn subscribe_receipts(&self) -> tokio::sync::broadcast::Receiver<QaBarrierReceiptEvent> {
        self.receipt_tx.subscribe()
    }

    pub async fn await_held_event(
        rx: &mut tokio::sync::broadcast::Receiver<QaBarrierHeldEvent>,
        expected_name: &str,
    ) -> QaBarrierHeldEvent {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.name == expected_name => return ev,
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("held event broadcast error: {e}"),
            }
        }
    }

    pub async fn await_receipt_event(
        rx: &mut tokio::sync::broadcast::Receiver<QaBarrierReceiptEvent>,
        expected_name: &str,
        expected_line: usize,
    ) -> Value {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.name == expected_name && ev.line_index == expected_line => {
                    return ev.payload
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("receipt event broadcast error: {e}"),
            }
        }
    }

    /// `fixture-setup` settlement: enumerates the private fixture sessions the
    /// headless harness drives. Line 0 is what the runner awaits before any
    /// trigger.
    pub fn emit_fixture_setup(&self, sessions: &[&str]) {
        let Some(operation_id) = self.operation_id() else {
            return;
        };
        self.append_receipt(
            "fixture-setup",
            &operation_id,
            json!({
                "sessionId": sessions.first().copied().unwrap_or_default(),
                "sessions": sessions,
                "fixtureKind": "headless-diagnostic-classifier",
            }),
        );
    }

    /// Headless fixture setup with explicit platform blocker disclosure.
    pub fn emit_fixture_setup_with_blocker(&self, sessions: &[&str], blocker: &str) {
        let Some(operation_id) = self.operation_id() else {
            return;
        };
        self.append_receipt(
            "fixture-setup",
            &operation_id,
            json!({
                "sessionId": sessions.first().copied().unwrap_or_default(),
                "sessions": sessions,
                "fixtureKind": "headless-diagnostic-classifier",
                "platformBlocker": blocker,
            }),
        );
    }

    /// Full typed 4-kind fixture setup emission conforming to pane-liveness.mjs.
    pub fn emit_fixture_setup_records(&self, records: &[FixtureSessionRecord]) {
        let Some(operation_id) = self.operation_id() else {
            return;
        };
        let first_id = records
            .first()
            .map(|r| r.backend_session_id.as_str())
            .unwrap_or_default();
        self.append_receipt(
            "fixture-setup",
            &operation_id,
            json!({
                "sessionId": first_id,
                "sessions": records,
                "fixtureKind": "real-isolated-pty-fixtures",
            }),
        );
    }

    fn snapshot_json(snapshot: &PaneLivenessSnapshot) -> Value {
        serde_json::to_value(snapshot).unwrap_or(Value::Null)
    }

    fn evidence_missing_fields(snapshot: &PaneLivenessSnapshot) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if snapshot.daemon_epoch.is_none() {
            missing.push("daemonEpoch");
        }
        if snapshot.reader_paused.is_none() {
            missing.push("readerPaused");
        }
        if snapshot.kernel_stopped.is_none() {
            missing.push("kernelStopped");
        }
        if snapshot.suspended.is_none() {
            missing.push("suspended");
        }
        if snapshot.presentation_receipt_received.is_none() {
            missing.push("presentationReceiptReceived");
        }
        missing
    }
}

/// Real collector snapshot for a session attached to surface host state. The
/// write-stage evidence (stage + pending age + success) is measured on the
/// REAL producer future, never invented.
fn write_stage_snapshot(
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    operation_id: &str,
    stage: Option<&str>,
    write_pending_ms: Option<u64>,
    write_success: Option<bool>,
) -> PaneLivenessSnapshot {
    let mut snapshot = state
        .session_liveness_observation(session_id)
        .unwrap_or_default();
    snapshot.telemetry_available = true;
    snapshot.session_id = Some(session_id.to_string());
    snapshot.vt_session_id = Some(session_id.to_string());
    snapshot.operation_id = Some(operation_id.to_string());
    snapshot.stage = stage.map(str::to_string);
    snapshot.write_pending_ms = write_pending_ms;
    snapshot.write_success = write_success;
    snapshot
}

pub(crate) fn verdict_str(verdict: PaneLivenessVerdict) -> String {
    // Runner contract: the frozen scenario asserts the PascalCase variant
    // names (`BlockedInIpcWrite`, `BlockedInPresentation`, `Idle`, ...).
    // `PaneLivenessVerdict` itself serializes SCREAMING_SNAKE on the wire;
    // the embedded `snapshot` object keeps that casing, the classifier
    // verdict field uses the runner's.
    format!("{verdict:?}")
}

/// Hold the REAL backend-write stage: called from
/// `send_native_terminal_input_with_stage_logging` after the
/// `backend_write_start` stage event and before the write future is awaited.
/// Returns `Some(outcome)` only when this exact operation drove the barrier.
pub(crate) async fn hold_backend_write_barrier(
    channel: &QaBarrierChannel,
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    operation_id: Option<&str>,
) -> Option<ReleaseOutcome> {
    let spec = channel.spec(WRITE_BARRIER)?;
    // Never fabricate an operation identity: only the runner's operation nonce
    // (carried by the arm and echoed by the request id) can hold the barrier.
    let operation_id = operation_id?;
    if operation_id != spec.operation_id {
        return None;
    }
    let start = tokio::time::Instant::now();
    // The real pending age must pass the classifier's slow-execution threshold
    // so the held verdict is measured evidence of a genuinely parked write.
    let threshold = start + std::time::Duration::from_millis(WRITE_PENDING_THRESHOLD_GRACE_MS);
    if tokio::time::Instant::now() < threshold {
        tokio::time::sleep_until(threshold).await;
    }
    let pending_ms = start.elapsed().as_millis() as u64;
    let held_snapshot = write_stage_snapshot(
        state,
        session_id,
        operation_id,
        Some("backend_write_start"),
        Some(pending_ms),
        None,
    );
    let held_verdict = classify_pane_liveness(&held_snapshot);
    channel.write_held(
        &spec,
        session_id,
        "backend_write_start",
        json!({
            "writePendingMs": pending_ms,
            "classifierVerdict": verdict_str(held_verdict),
        }),
    );
    channel.append_receipt(
        WRITE_BARRIER,
        operation_id,
        json!({
            "sessionId": session_id,
            "stage": "backend_write_start",
            "classifierVerdict": verdict_str(held_verdict),
            "writePendingMs": pending_ms,
            "snapshot": QaBarrierChannel::snapshot_json(&held_snapshot),
        }),
    );
    Some(channel.wait_for_release(&spec).await)
}

/// Settle the backend-write barrier after the real write future completed.
/// The receipt carries the fresh collector snapshot plus actual stage-progress
/// evidence; the verdict is whatever the real classifier returns from that
/// snapshot. `Idle` is never forced - an `Unknown` verdict is reported with
/// `evidenceMissing: true` and the exact missing-field list.
pub(crate) fn settle_backend_write_barrier(
    channel: &QaBarrierChannel,
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    operation_id: Option<&str>,
    outcome: Option<ReleaseOutcome>,
    success: bool,
    duration_ms: f64,
) {
    let Some(spec) = channel.spec(WRITE_BARRIER) else {
        return;
    };
    let Some(operation_id) = operation_id else {
        return;
    };
    if operation_id != spec.operation_id {
        return;
    }
    let fresh_snapshot =
        write_stage_snapshot(state, session_id, operation_id, None, None, Some(success));
    let verdict = classify_pane_liveness(&fresh_snapshot);
    let missing = QaBarrierChannel::evidence_missing_fields(&fresh_snapshot);
    channel.append_receipt(
        WRITE_BARRIER,
        operation_id,
        json!({
            "sessionId": session_id,
            "stage": "backend_write_settled",
            "classifierVerdict": verdict_str(verdict),
            "evidenceMissing": verdict == PaneLivenessVerdict::Unknown,
            "evidenceMissingFields": missing,
            "stageProgress": {
                "backendWriteCompleted": true,
                "success": success,
                "durationMs": duration_ms,
                "releaseOutcome": outcome.map(ReleaseOutcome::as_str),
            },
            "snapshot": QaBarrierChannel::snapshot_json(&fresh_snapshot),
        }),
    );
}

/// Headless dispatch entry: `ferryx diagnostic-classifier --headless`.
///
/// Must run BEFORE any GUI/daemon routing (see `main.rs`). Exercises the real
/// Task 2 write-stage and presentation coordinator producer paths in process
/// without windows. Actual native presentation proof is explicitly deferred
/// to task 10: the emitted summary records `nativeEvidence:
/// "deferred-to-task-10"` and `presentationEvidence: "coordinator-consumed"`.
pub fn run_diagnostic_classifier_headless() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    let headless_argv = args.len() == 3
        && args[1] == "diagnostic-classifier"
        && args[2] == "--headless";
    if !headless_argv {
        eprintln!(
            "FERRYX_QA_INVALID_INVOCATION: expected exactly `diagnostic-classifier --headless`"
        );
        return 2;
    }
    let channel = match QaBarrierChannel::from_env() {
        Ok(channel) => channel,
        Err(error) => {
            eprintln!("FERRYX_QA_CHANNEL_UNAVAILABLE: {error}");
            return 2;
        }
    };
    install(channel);
    let Some(channel) = active_channel() else {
        eprintln!("FERRYX_QA_CHANNEL_UNAVAILABLE: channel installation failed");
        return 2;
    };
    let (acked, rejected) = channel.scan_and_ack_arms();
    if !rejected.is_empty() {
        eprintln!("FERRYX_QA_ARM_REJECTED: rejected prelaunch arms: {rejected:?}");
        deactivate();
        return 2;
    }
    for required in [WRITE_BARRIER, PRESENTATION_BARRIER] {
        if !acked.iter().any(|name| name == required) {
            eprintln!(
                "FERRYX_QA_BARRIER_NOT_ARMED: barrier {required} was not armed by the runner (acked={acked:?} rejected={rejected:?})"
            );
            deactivate();
            return 2;
        }
    }
    // Task 3 Unit 3-A: Real isolated PTY fixture generation.
    // Retain _fixture_guard across the entire scenario lifetime so spawned
    // fixture sessions and kernel-stopped processes are preserved until
    // settlement and then cleaned up boundedly.
    // The write stage and presentation coordinator run on the ACTUAL real
    // fixture sessions rather than hardcoded mock strings.
    let (write_session, presentation_session, _fixture_guard) = match build_isolated_fixture_sessions_sync() {
        Ok((records, guard)) => {
            channel.emit_fixture_setup_records(&records);
            let write_id = records
                .iter()
                .find(|r| r.kind == "created")
                .map(|r| r.backend_session_id.clone())
                .unwrap_or_else(|| "qa-headless-write".to_string());
            let pres_id = records
                .iter()
                .find(|r| r.kind == "idle")
                .map(|r| r.backend_session_id.clone())
                .unwrap_or_else(|| "qa-headless-presentation".to_string());
            (write_id, pres_id, Some(guard))
        }
        Err(FixtureError::PlatformUnsupported(reason)) => {
            let write_id = "qa-headless-write".to_string();
            let pres_id = "qa-headless-presentation".to_string();
            channel.emit_fixture_setup_with_blocker(
                &[&write_id, &pres_id],
                &reason,
            );
            (write_id, pres_id, None)
        }
        Err(error) => {
            deactivate();
            eprintln!("FERRYX_QA_FIXTURE_SETUP_FAILURE: {error}");
            return 1;
        }
    };

    let pty_manager = _fixture_guard.as_ref().map(|g| g.pty_manager().clone());

    match run_headless_stages(&channel, &write_session, &presentation_session, pty_manager) {
        Ok(summary) => {
            // Settle / teardown producer fixtures first
            drop(_fixture_guard);

            // Now seal and drain all owned receipt workers, propagating any failures
            let drain_res = {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(rt) => rt.block_on(channel.drain_and_verify_workers()),
                    Err(e) => Err(format!("worker drain runtime: {e}")),
                }
            };
            if let Err(e) = drain_res {
                deactivate();
                eprintln!("FERRYX_QA_RECEIPT_WORKER_DRAIN_FAILURE: {e}");
                return 1;
            }

            deactivate();
            println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
            0
        }
        Err(error) => {
            drop(_fixture_guard);
            deactivate();
            eprintln!("FERRYX_QA_HEADLESS_FAILURE: {error}");
            1
        }
    }
}

fn run_headless_stages(
    channel: &Arc<QaBarrierChannel>,
    write_session: &str,
    presentation_session: &str,
    pty_manager: Option<Arc<crate::terminal::PtyManager>>,
) -> Result<Value, String> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?;
    runtime.block_on(async move {
        // Real write-stage producer through the production pipeline, exactly
        // like the frozen R9 test
        // `pane_liveness_diagnostics_prearmed_producer_write_barrier` but
        // driven by the private barrier channel instead of oneshots.
        let app = tauri::test::mock_builder()
            .manage(NativeTerminalSurfaceHostState::default())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .map_err(|e| format!("mock app: {e}"))?;
        let state = app.state::<NativeTerminalSurfaceHostState>();
        let (output_tx, messages) = tokio::sync::mpsc::channel(1);
        // Keep the sender alive like the frozen R9 fixture so the pump sees
        // an open stream for the whole headless run.
        let _keep_output_tx = output_tx;
        let stream_task = tokio::spawn(std::future::pending());
        state
            .attach_daemon_attachment_with_bounds::<tauri::test::MockRuntime>(
                write_session,
                crate::daemon::DaemonAttachment {
                    session_id: write_session.to_string(),
                    epoch: 1,
                    start_sequence: Some(1),
                    end_sequence: Some(1),
                    gap: None,
                    history: bytes::Bytes::new(),
                    history_segments: Vec::new(),
                    pty_cols: Some(80),
                    pty_rows: Some(24),
                    remote_generation: None,
                    messages,
                    stream_task,
                },
                Some(app.handle().clone()),
                None,
            )
            .map_err(|e| format!("attach write session: {e}"))?;

        let operation_id = channel
            .operation_id()
            .ok_or_else(|| "no armed barrier operation nonce".to_string())?;
        let input = crate::native_terminal::NativeTerminalInput::Text {
            // Private fixture only; bytes are never logged into diagnostics.
            text: "ferryx-qa-headless-write-probe".to_string(),
        };
        let pty = pty_manager.clone();
        let sid = write_session.to_string();
        crate::ipc::native_terminal::send_native_terminal_input_with_stage_logging(
            &app.handle(),
            &state,
            write_session,
            &input,
            Some(1),
            Some(operation_id),
            move |bytes| {
                let pty = pty.clone();
                let sid = sid.clone();
                async move {
                    if let Some(ref mgr) = pty {
                        mgr.write_input(&sid, bytes)
                            .map_err(|e| format!("pty write_input: {e}"))
                    } else {
                        Ok(())
                    }
                }
            },
        )
        .await
        .map_err(|e| format!("write stage: {e}"))?;

        // Real presentation-coordinator producer: a scheduled frame held
        // pending, classified through the real collector, then consumed.
        let coordinator = state
            .attach_test_session_for_liveness(presentation_session, 1, Some(42))
            .map_err(|e| format!("attach presentation session: {e}"))?;
        let presentation_outcome = state
            .hold_presentation_barrier_qa(&coordinator, presentation_session, channel)
            .await;

        state.teardown();
        Ok(json!({
            "runId": channel.run_id(),
            "scenario": "diagnostic-classifier",
            "mode": "headless",
            "verdict": "DEFERRED-NATIVE",
            "nativeEvidence": "deferred-to-task-10",
            "presentationEvidence": "coordinator-consumed",
            "writeStage": {
                "sessionId": write_session,
                "producer": "ipc-native-terminal",
            },
            "presentationStage": {
                "sessionId": presentation_session,
                "producer": NativeTerminalSurfaceHostState::PRESENTATION_PRODUCER_ID,
                "releaseOutcome": presentation_outcome.map(ReleaseOutcome::as_str),
            },
        }))
    })
}

// ============================================================================
// Task 3 Unit 3-A: Typed Fixture Session Records & Real Isolated PTY Generation
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct FixtureSessionRecord {
    pub kind: String,
    #[serde(rename = "backendSessionId")]
    pub backend_session_id: String,
    #[serde(rename = "ownershipReceipt")]
    pub ownership_receipt: Value,
    #[serde(rename = "stopProbeState", skip_serializing_if = "Option::is_none")]
    pub stop_probe_state: Option<String>,
    #[serde(rename = "originalBackendSessionId", skip_serializing_if = "Option::is_none")]
    pub original_backend_session_id: Option<String>,
    #[serde(rename = "adoptedBackendSessionId", skip_serializing_if = "Option::is_none")]
    pub adopted_backend_session_id: Option<String>,
    #[serde(rename = "originalIncarnation", skip_serializing_if = "Option::is_none")]
    pub original_incarnation: Option<String>,
    #[serde(rename = "adoptedIncarnation", skip_serializing_if = "Option::is_none")]
    pub adopted_incarnation: Option<String>,
    #[serde(rename = "incarnationStatus", skip_serializing_if = "Option::is_none")]
    pub incarnation_status: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlatformGateReport {
    pub pty_export_supported: bool,
    pub kernel_stop_supported: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker_reason: Option<String>,
}

impl PlatformGateReport {
    pub fn current() -> Self {
        #[cfg(unix)]
        {
            Self {
                pty_export_supported: true,
                kernel_stop_supported: true,
                blocker_reason: None,
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                pty_export_supported: false,
                kernel_stop_supported: false,
                blocker_reason: Some(
                    "Windows baseline does not support POSIX PTY descriptor transfer or SIGSTOP kernel probe"
                        .to_string(),
                ),
            }
        }
    }
}

#[derive(Debug)]
pub enum FixtureError {
    Spawn(String),
    #[cfg(unix)]
    Export(String),
    #[cfg(unix)]
    Adopt(String),
    SafetyViolation(String),
    #[cfg(unix)]
    Signal(String),
    #[cfg(unix)]
    Probe(String),
    PlatformUnsupported(String),
    Ipc(String),
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(s) => write!(f, "PTY spawn failed: {s}"),
            #[cfg(unix)]
            Self::Export(s) => write!(f, "PTY export failed: {s}"),
            #[cfg(unix)]
            Self::Adopt(s) => write!(f, "PTY adopt failed: {s}"),
            Self::SafetyViolation(s) => write!(f, "Safety violation: {s}"),
            #[cfg(unix)]
            Self::Signal(s) => write!(f, "Process signal failed: {s}"),
            #[cfg(unix)]
            Self::Probe(s) => write!(f, "Process probe failed: {s}"),
            Self::PlatformUnsupported(s) => write!(f, "Platform unsupported: {s}"),
            Self::Ipc(s) => write!(f, "IPC error: {s}"),
        }
    }
}

impl std::error::Error for FixtureError {}

pub struct IsolatedFixtureGuard {
    pub pty_manager: Arc<crate::terminal::PtyManager>,
    pub created_id: Option<String>,
    pub idle_id: Option<String>,
    #[cfg(unix)]
    pub stopped_id: Option<String>,
    #[cfg(unix)]
    pub stopped_pid: Option<u32>,
    #[cfg(unix)]
    pub pred_manager: Option<Arc<crate::terminal::PtyManager>>,
    #[cfg(unix)]
    pub succ_manager: Option<Arc<crate::terminal::PtyManager>>,
    #[cfg(unix)]
    pub adopted_id: Option<String>,
}

impl IsolatedFixtureGuard {
    pub fn pty_manager(&self) -> &Arc<crate::terminal::PtyManager> {
        &self.pty_manager
    }
}

impl Drop for IsolatedFixtureGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if let Some(pid) = self.stopped_pid {
                if pid > 1 && pid != std::process::id() {
                    unsafe {
                        libc::kill(pid as libc::pid_t, libc::SIGCONT);
                    }
                }
            }
            if let Some(ref id) = self.stopped_id {
                let _ = self.pty_manager.kill(id);
            }
            // Adopted session ownership was transferred to successor manager.
            // ONLY successor manager should terminate the active child process.
            // Predecessor manager relinquished its handle during transfer;
            // signaling via predecessor would erroneously kill the process that successor owns.
            if let (Some(ref succ), Some(ref id)) = (&self.succ_manager, &self.adopted_id) {
                let _ = succ.kill(id);
            }
        }
        if let Some(ref id) = self.created_id {
            let _ = self.pty_manager.kill(id);
        }
        if let Some(ref id) = self.idle_id {
            let _ = self.pty_manager.kill(id);
        }
    }
}

#[cfg(target_os = "macos")]
fn prearm_and_await_child_stop(
    sess: &Arc<crate::terminal::PtySession>,
    pid: u32,
) -> Result<(), FixtureError> {
    let kq = unsafe { libc::kqueue() };
    if kq < 0 {
        return Err(FixtureError::Probe(format!(
            "kqueue creation failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    struct KqGuard(libc::c_int);
    impl Drop for KqGuard {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.0);
            }
        }
    }
    let _guard = KqGuard(kq);

    // Darwin EVFILT_PROC with NOTE_SIGNAL (0x08000000):
    // Monitors signal delivery to child without calling waitpid or stealing status.
    let event = libc::kevent {
        ident: pid as usize,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
        fflags: libc::NOTE_SIGNAL,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    let ret = unsafe {
        libc::kevent(
            kq,
            &event,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    if ret < 0 {
        return Err(FixtureError::Probe(format!(
            "kevent prearm failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    let sig_ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) };
    if sig_ret != 0 {
        return Err(FixtureError::Signal(format!(
            "SIGSTOP failed on PID {pid}: {}",
            std::io::Error::last_os_error()
        )));
    }

    let timeout = libc::timespec {
        tv_sec: 5,
        tv_nsec: 0,
    };
    let mut out_event: libc::kevent = unsafe { std::mem::zeroed() };
    let nev = unsafe {
        libc::kevent(
            kq,
            std::ptr::null(),
            0,
            &mut out_event,
            1,
            &timeout,
        )
    };
    if nev <= 0 {
        return Err(FixtureError::Probe(format!(
            "bounded OS child signal event timed out for PID {pid}"
        )));
    }

    if !sess.process_stopped() {
        return Err(FixtureError::Probe(format!(
            "kernel kinfo_proc does not report PID {pid} stopped"
        )));
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn prearm_and_await_child_stop(
    sess: &Arc<crate::terminal::PtySession>,
    pid: u32,
) -> Result<(), FixtureError> {
    let sig_ret = unsafe { libc::kill(pid as libc::pid_t, libc::SIGSTOP) };
    if sig_ret != 0 {
        return Err(FixtureError::Signal(format!(
            "SIGSTOP failed on PID {pid}: {}",
            std::io::Error::last_os_error()
        )));
    }
    if !sess.process_stopped() {
        return Err(FixtureError::Probe(format!(
            "Linux /proc/{pid}/stat does not report stopped state 'T'"
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn prearm_and_await_child_stop(
    _sess: &Arc<crate::terminal::PtySession>,
    _pid: u32,
) -> Result<(), FixtureError> {
    Err(FixtureError::PlatformUnsupported(
        "Windows ConPTY baseline does not support POSIX SIGSTOP or child stop events".to_string(),
    ))
}

/// Builds typed fixture ownership records from real isolated owned PTY sessions.
///
/// On Unix, creates real isolated PTYs for created, idle, adopted (via baseline
/// export/adoption APIs), and externally-stopped (via SIGSTOP verified by kernel probe).
/// On Windows, returns explicit `FixtureError::PlatformUnsupported` detailing the
/// concrete platform gate blocker rather than fabricating Unix behavior.
pub fn build_isolated_fixture_sessions_sync() -> Result<(Vec<FixtureSessionRecord>, IsolatedFixtureGuard), FixtureError> {
    #[cfg(not(unix))]
    {
        return Err(FixtureError::PlatformUnsupported(
            "Windows baseline does not support POSIX PTY descriptor transfer or SIGSTOP kernel probe. Unit 3-A preserves concrete blocker.".to_string()
        ));
    }

    #[cfg(unix)]
    {
        let pty_manager = Arc::new(crate::terminal::PtyManager::new());

        // 1. created fixture
        let (created_id, _) = pty_manager
            .spawn_shell(80, 24)
            .map_err(|e| FixtureError::Spawn(format!("created: {e}")))?;
        let created_pid = pty_manager.get_session(&created_id).and_then(|s| s.pid());
        let created_record = FixtureSessionRecord {
            kind: "created".to_string(),
            backend_session_id: created_id.clone(),
            ownership_receipt: json!({
                "producer": PRODUCER_ID,
                "producerPid": std::process::id(),
                "leafId": "fixture-created",
                "epoch": 1,
                "incarnation": Value::Null,
                "incarnationStatus": "baseline-unavailable-task-4-gate",
                "ptyPid": created_pid,
            }),
            stop_probe_state: None,
            original_backend_session_id: None,
            adopted_backend_session_id: None,
            original_incarnation: None,
            adopted_incarnation: None,
            incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
        };

        // 2. idle fixture
        let (idle_id, _) = pty_manager
            .spawn_shell(80, 24)
            .map_err(|e| FixtureError::Spawn(format!("idle: {e}")))?;
        let idle_pid = pty_manager.get_session(&idle_id).and_then(|s| s.pid());
        let idle_record = FixtureSessionRecord {
            kind: "idle".to_string(),
            backend_session_id: idle_id.clone(),
            ownership_receipt: json!({
                "producer": PRODUCER_ID,
                "producerPid": std::process::id(),
                "leafId": "fixture-idle",
                "epoch": 1,
                "incarnation": Value::Null,
                "incarnationStatus": "baseline-unavailable-task-4-gate",
                "ptyPid": idle_pid,
            }),
            stop_probe_state: None,
            original_backend_session_id: None,
            adopted_backend_session_id: None,
            original_incarnation: None,
            adopted_incarnation: None,
            incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
        };

        // 3. adopted fixture using real baseline transfer / adoption APIs
        let pred_manager = Arc::new(crate::terminal::PtyManager::new());
        let (pred_id, _) = pred_manager
            .spawn_shell(80, 24)
            .map_err(|e| FixtureError::Spawn(format!("adopted predecessor: {e}")))?;
        let export = pred_manager
            .export_session(&pred_id)
            .map_err(|e| FixtureError::Export(format!("adopted export: {e}")))?;
        let succ_manager = Arc::new(crate::terminal::PtyManager::new());
        let _rx = succ_manager
            .adopt_transferred_export(export)
            .map_err(|e| FixtureError::Adopt(format!("adopted adoption: {e}")))?;
        let adopted_record = FixtureSessionRecord {
            kind: "adopted".to_string(),
            backend_session_id: pred_id.clone(),
            ownership_receipt: json!({
                "producer": PRODUCER_ID,
                "producerPid": std::process::id(),
                "leafId": "fixture-adopted",
                "epoch": 2,
                "incarnation": Value::Null,
                "incarnationStatus": "baseline-unavailable-task-4-gate",
                "adopted": true,
            }),
            stop_probe_state: None,
            original_backend_session_id: Some(pred_id.clone()),
            adopted_backend_session_id: Some(pred_id.clone()),
            original_incarnation: None,
            adopted_incarnation: None,
            incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
        };

        // 4. externally-stopped fixture: real SIGSTOP + kernel observation
        let (stopped_id, _) = pty_manager
            .spawn_shell(80, 24)
            .map_err(|e| FixtureError::Spawn(format!("stopped: {e}")))?;
        let stopped_sess = pty_manager
            .get_session(&stopped_id)
            .ok_or_else(|| FixtureError::Spawn("stopped session not found".into()))?;
        let stopped_pid = stopped_sess
            .pid()
            .ok_or_else(|| FixtureError::Spawn("missing child PID for stopped fixture".into()))?;

        // Safety verification: verify PID > 1, PID != self
        if stopped_pid <= 1 || stopped_pid == std::process::id() {
            return Err(FixtureError::SafetyViolation(format!(
                "invalid child PID for SIGSTOP: {stopped_pid}"
            )));
        }

        prearm_and_await_child_stop(&stopped_sess, stopped_pid)?;

        let stopped_record = FixtureSessionRecord {
            kind: "externally-stopped".to_string(),
            backend_session_id: stopped_id.clone(),
            ownership_receipt: json!({
                "producer": PRODUCER_ID,
                "producerPid": std::process::id(),
                "leafId": "fixture-externally-stopped",
                "epoch": 1,
                "incarnation": Value::Null,
                "incarnationStatus": "baseline-unavailable-task-4-gate",
                "ptyPid": stopped_pid,
                "stoppedBySignal": "SIGSTOP",
            }),
            stop_probe_state: Some("stopped".to_string()),
            original_backend_session_id: None,
            adopted_backend_session_id: None,
            original_incarnation: None,
            adopted_incarnation: None,
            incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
        };

        let records = vec![
            created_record,
            adopted_record,
            stopped_record,
            idle_record,
        ];

        let guard = IsolatedFixtureGuard {
            pty_manager,
            created_id: Some(created_id),
            idle_id: Some(idle_id),
            stopped_id: Some(stopped_id),
            stopped_pid: Some(stopped_pid),
            pred_manager: Some(pred_manager),
            succ_manager: Some(succ_manager),
            adopted_id: Some(pred_id),
        };

        Ok((records, guard))
    }
}

pub async fn build_isolated_fixture_sessions() -> Result<(Vec<FixtureSessionRecord>, IsolatedFixtureGuard), FixtureError> {
    match crate::ipc::run_blocking(|| {
        build_isolated_fixture_sessions_sync()
    })
    .await
    {
        Ok(inner_res) => inner_res,
        Err(ipc_err) => Err(FixtureError::Ipc(ipc_err.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_RUN_ID: &str = "qa-run-test";
    const TEST_OPERATION_ID: &str = "qa-op-test";

    fn arm(dir: &std::path::Path, name: &str, run_id: &str, operation_id: &str) {
        std::fs::write(
            dir.join(format!("{name}.arm.json")),
            serde_json::to_string_pretty(&json!({
                "name": name,
                "runId": run_id,
                "operationId": operation_id,
                "deadlineMs": 5_000,
                "plan": null,
                "armedAt": "2026-10-03T00:00:00.000Z",
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn release(dir: &std::path::Path, name: &str, run_id: &str, operation_id: &str) {
        std::fs::write(
            dir.join(format!("{name}.release.json")),
            serde_json::to_string_pretty(&json!({
                "name": name,
                "runId": run_id,
                "operationId": operation_id,
                "releasedAt": "2026-10-03T00:00:01.000Z",
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn read_lines(dir: &std::path::Path, name: &str) -> Vec<Value> {
        let text = std::fs::read_to_string(dir.join(format!("{name}.receipt.jsonl"))).unwrap();
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    // Startup registration race: arms written BEFORE the channel starts are
    // all acked; a wrong-nonce arm is never acked and never holdable.
    #[test]
    fn startup_scan_acks_prearmed_barriers_and_rejects_wrong_nonce() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        arm(&dir, PRESENTATION_BARRIER, "qa-run-OTHER", TEST_OPERATION_ID);

        let channel = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        let (acked, rejected) = channel.scan_and_ack_arms();

        assert_eq!(acked, vec![WRITE_BARRIER.to_string()]);
        assert_eq!(
            rejected,
            vec![format!("{}:run-nonce-mismatch", PRESENTATION_BARRIER)]
        );
        let ack: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{WRITE_BARRIER}.armed-ack.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(ack["runId"], json!(TEST_RUN_ID));
        assert_eq!(ack["operationId"], json!(TEST_OPERATION_ID));
        assert_eq!(ack["producer"], json!(PRODUCER_ID));
        assert!(ack["producerPid"].as_u64().is_some());
        assert!(!dir.join(format!("{PRESENTATION_BARRIER}.armed-ack.json")).exists());
        // The rejected arm is not registered, so it can never be held.
        assert!(channel.spec(PRESENTATION_BARRIER).is_none());
    }

    // A release control with the wrong run or operation identity is ignored;
    // only the correlated control releases the hold.
    #[tokio::test(start_paused = true)]
    async fn release_requires_matching_run_and_operation_identity() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        channel.scan_and_ack_arms();
        let spec = channel.spec(WRITE_BARRIER).unwrap();

        release(&dir, WRITE_BARRIER, TEST_RUN_ID, "qa-op-OTHER");
        let held = tokio::task::spawn(async move {
            channel.wait_for_release(&spec).await
        });
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!held.is_finished(), "wrong-operation release must be ignored");
        release(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        assert_eq!(
            held.await.unwrap(),
            ReleaseOutcome::Released,
            "correlated release must settle the hold"
        );
        assert!(dir.join(format!("{WRITE_BARRIER}.rejected-control.json")).exists());
    }

    // Bounded cancellation: without a release the hold returns at the arm
    // deadline instead of parking the producer forever.
    #[tokio::test(start_paused = true)]
    async fn hold_deadline_bounds_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{WRITE_BARRIER}.arm.json")),
            serde_json::to_string(&json!({
                "name": WRITE_BARRIER,
                "runId": TEST_RUN_ID,
                "operationId": TEST_OPERATION_ID,
                "deadlineMs": 1_000,
            }))
            .unwrap(),
        )
        .unwrap();
        let channel = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        channel.scan_and_ack_arms();
        let spec = channel.spec(WRITE_BARRIER).unwrap();
        let started = tokio::time::Instant::now();
        assert_eq!(
            channel.wait_for_release(&spec).await,
            ReleaseOutcome::DeadlineExceeded
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(1_000),
            "deadline must bound the wait"
        );
    }

    // Fixture settlement line 0 carries the full correlation triple.
    #[test]
    fn fixture_setup_receipt_carries_correlation() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        channel.scan_and_ack_arms();
        channel.emit_fixture_setup(&["qa-a", "qa-b"]);
        let lines = read_lines(&dir, "fixture-setup");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["runId"], json!(TEST_RUN_ID));
        assert_eq!(lines[0]["operationId"], json!(TEST_OPERATION_ID));
        assert_eq!(lines[0]["producer"], json!(PRODUCER_ID));
        assert!(lines[0]["producerPid"].as_u64().is_some());
        assert_eq!(lines[0]["sessions"], json!(["qa-a", "qa-b"]));
    }

    // No env/channel: active_channel stays None; the production write path is
    // untouched (also asserted in the native_terminal writer tests).
    #[test]
    fn channel_absent_without_install() {
        deactivate();
        assert!(active_channel().is_none());
    }

    #[tokio::test]
    async fn barrier_constants_and_access_apis_are_armed_and_held_deterministically() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();

        for barrier in [
            ATTACH_HANDSHAKE_BARRIER,
            PREDECESSOR_EXPORT_BARRIER,
            SUCCESSOR_ADOPT_BARRIER,
            COMMIT_BARRIER,
            ABORT_BARRIER,
            HELD_RPC_BARRIER,
        ] {
            arm(&dir, barrier, TEST_RUN_ID, TEST_OPERATION_ID);
        }

        let channel = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        let (acked, rejected) = channel.scan_and_ack_arms();
        assert_eq!(acked.len(), 6);
        assert!(rejected.is_empty());

        for barrier in [
            ATTACH_HANDSHAKE_BARRIER,
            PREDECESSOR_EXPORT_BARRIER,
            SUCCESSOR_ADOPT_BARRIER,
            COMMIT_BARRIER,
            ABORT_BARRIER,
            HELD_RPC_BARRIER,
        ] {
            assert!(channel.is_armed(barrier));
            assert!(channel.spec(barrier).is_some());
        }

        let mut held_rx = channel.subscribe_held();
        let mut receipt_rx = channel.subscribe_receipts();

        let hold_fut = channel.hold_barrier(
            ATTACH_HANDSHAKE_BARRIER,
            "session-attach-test",
            "attach_pending",
            json!({ "test": true }),
        );
        tokio::pin!(hold_fut);

        let held = QaBarrierChannel::await_held_event(&mut held_rx, ATTACH_HANDSHAKE_BARRIER).await;
        assert_eq!(held.name, ATTACH_HANDSHAKE_BARRIER);
        assert_eq!(held.session_id, "session-attach-test");
        assert_eq!(held.stage, "attach_pending");

        release(&dir, ATTACH_HANDSHAKE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let outcome = hold_fut.await;
        assert_eq!(outcome, Some(ReleaseOutcome::Released));

        channel.append_receipt(
            ATTACH_HANDSHAKE_BARRIER,
            TEST_OPERATION_ID,
            json!({ "settled": true }),
        );
        let receipt = QaBarrierChannel::await_receipt_event(&mut receipt_rx, ATTACH_HANDSHAKE_BARRIER, 1).await;
        assert_eq!(receipt["settled"], json!(true));
    }

    #[test]
    fn platform_gate_reports_real_platform_capabilities() {
        let report = PlatformGateReport::current();
        #[cfg(unix)]
        {
            assert!(report.pty_export_supported);
            assert!(report.kernel_stop_supported);
            assert!(report.blocker_reason.is_none());
        }
        #[cfg(not(unix))]
        {
            assert!(!report.pty_export_supported);
            assert!(!report.kernel_stop_supported);
            assert!(report.blocker_reason.is_some());
        }
    }

    #[test]
    fn isolated_pty_fixtures_real_spawning_and_bounded_cleanup() {
        let res = build_isolated_fixture_sessions_sync();
        #[cfg(unix)]
        {
            let (records, guard) = res.expect("isolated fixtures spawn successfully on unix");
            assert_eq!(records.len(), 4);
            let kinds: Vec<&str> = records.iter().map(|r| r.kind.as_str()).collect();
            assert_eq!(kinds, vec!["created", "adopted", "externally-stopped", "idle"]);

            let stopped = records.iter().find(|r| r.kind == "externally-stopped").unwrap();
            assert_eq!(stopped.stop_probe_state.as_deref(), Some("stopped"));

            let adopted = records.iter().find(|r| r.kind == "adopted").unwrap();
            assert_eq!(adopted.original_backend_session_id, adopted.adopted_backend_session_id);
            assert_eq!(adopted.original_incarnation, None);
            assert_eq!(adopted.adopted_incarnation, None);
            assert_eq!(
                adopted.incarnation_status.as_deref(),
                Some("baseline-unavailable-task-4-gate")
            );

            drop(guard);
        }
        #[cfg(not(unix))]
        {
            match res {
                Err(FixtureError::PlatformUnsupported(reason)) => {
                    assert!(reason.contains("Windows baseline does not support"));
                }
                other => panic!("expected PlatformUnsupported on Windows, got {:?}", other.map(|(r, _)| r)),
            }
        }
    }

    #[test]
    fn emit_fixture_setup_records_emits_valid_jsonl_receipt() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);

        let channel = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        channel.scan_and_ack_arms();

        let records = vec![
            FixtureSessionRecord {
                kind: "created".to_string(),
                backend_session_id: "s-created".to_string(),
                ownership_receipt: json!({ "epoch": 1, "incarnationStatus": "baseline-unavailable-task-4-gate" }),
                stop_probe_state: None,
                original_backend_session_id: None,
                adopted_backend_session_id: None,
                original_incarnation: None,
                adopted_incarnation: None,
                incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
            },
            FixtureSessionRecord {
                kind: "adopted".to_string(),
                backend_session_id: "s-adopted".to_string(),
                ownership_receipt: json!({ "epoch": 2, "incarnationStatus": "baseline-unavailable-task-4-gate" }),
                stop_probe_state: None,
                original_backend_session_id: Some("s-adopted".into()),
                adopted_backend_session_id: Some("s-adopted".into()),
                original_incarnation: None,
                adopted_incarnation: None,
                incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
            },
            FixtureSessionRecord {
                kind: "externally-stopped".to_string(),
                backend_session_id: "s-stopped".to_string(),
                ownership_receipt: json!({ "epoch": 1, "incarnationStatus": "baseline-unavailable-task-4-gate" }),
                stop_probe_state: Some("stopped".to_string()),
                original_backend_session_id: None,
                adopted_backend_session_id: None,
                original_incarnation: None,
                adopted_incarnation: None,
                incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
            },
            FixtureSessionRecord {
                kind: "idle".to_string(),
                backend_session_id: "s-idle".to_string(),
                ownership_receipt: json!({ "epoch": 1, "incarnationStatus": "baseline-unavailable-task-4-gate" }),
                stop_probe_state: None,
                original_backend_session_id: None,
                adopted_backend_session_id: None,
                original_incarnation: None,
                adopted_incarnation: None,
                incarnation_status: Some("baseline-unavailable-task-4-gate".to_string()),
            },
        ];

        channel.emit_fixture_setup_records(&records);
        let lines = read_lines(&dir, "fixture-setup");
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line["runId"], json!(TEST_RUN_ID));
        assert_eq!(line["operationId"], json!(TEST_OPERATION_ID));
        assert_eq!(line["producer"], json!(PRODUCER_ID));
        assert_eq!(line["fixtureKind"], json!("real-isolated-pty-fixtures"));
        let sessions = line["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 4);
        assert_eq!(sessions[2]["kind"], json!("externally-stopped"));
        assert_eq!(sessions[2]["stopProbeState"], json!("stopped"));
    }

    #[test]
    fn role_scoped_arms_preserve_ownership_across_predecessor_and_successor() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();

        // Write arm targeted at predecessor
        std::fs::write(
            dir.join(format!("{PREDECESSOR_EXPORT_BARRIER}.arm.json")),
            serde_json::to_string(&json!({
                "name": PREDECESSOR_EXPORT_BARRIER,
                "runId": TEST_RUN_ID,
                "operationId": "op-pred-1",
                "deadlineMs": 5000,
                "targetRole": "predecessor",
            }))
            .unwrap(),
        )
        .unwrap();

        // Write arm targeted at successor
        std::fs::write(
            dir.join(format!("{SUCCESSOR_ADOPT_BARRIER}.arm.json")),
            serde_json::to_string(&json!({
                "name": SUCCESSOR_ADOPT_BARRIER,
                "runId": TEST_RUN_ID,
                "operationId": "op-succ-2",
                "deadlineMs": 5000,
                "targetRole": "successor",
            }))
            .unwrap(),
        )
        .unwrap();

        let pred_channel = QaBarrierChannel::new_with_role(
            dir.clone(),
            TEST_RUN_ID.to_string(),
            Some("predecessor".to_string()),
        );
        let (pred_acked, _) = pred_channel.scan_and_ack_arms();
        assert_eq!(pred_acked, vec![PREDECESSOR_EXPORT_BARRIER.to_string()]);
        assert_eq!(
            pred_channel.operation_id_for(PREDECESSOR_EXPORT_BARRIER).as_deref(),
            Some("op-pred-1")
        );
        assert!(pred_channel.spec(SUCCESSOR_ADOPT_BARRIER).is_none());

        let succ_channel = QaBarrierChannel::new_with_role(
            dir.clone(),
            TEST_RUN_ID.to_string(),
            Some("successor".to_string()),
        );
        let (succ_acked, _) = succ_channel.scan_and_ack_arms();
        assert_eq!(succ_acked, vec![SUCCESSOR_ADOPT_BARRIER.to_string()]);
        assert_eq!(
            succ_channel.operation_id_for(SUCCESSOR_ADOPT_BARRIER).as_deref(),
            Some("op-succ-2")
        );
        assert!(succ_channel.spec(PREDECESSOR_EXPORT_BARRIER).is_none());

        // Both role-specific acks exist and have distinct producer role identities
        assert!(dir.join(format!("{PREDECESSOR_EXPORT_BARRIER}.armed-ack.predecessor.json")).exists());
        assert!(dir.join(format!("{SUCCESSOR_ADOPT_BARRIER}.armed-ack.successor.json")).exists());
    }

    #[tokio::test]
    async fn target_backend_session_id_matching_and_bound_ack() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();

        // Arm with explicit targetBackendSessionId
        std::fs::write(
            dir.join(format!("{ATTACH_HANDSHAKE_BARRIER}.arm.json")),
            serde_json::to_string(&json!({
                "name": ATTACH_HANDSHAKE_BARRIER,
                "runId": TEST_RUN_ID,
                "operationId": "op-attach-1",
                "deadlineMs": 5000,
                "targetBackendSessionId": "sess-target-42",
            }))
            .unwrap(),
        )
        .unwrap();

        // Arm with NO targetBackendSessionId (None)
        std::fs::write(
            dir.join(format!("{WRITE_BARRIER}.arm.json")),
            serde_json::to_string(&json!({
                "name": WRITE_BARRIER,
                "runId": TEST_RUN_ID,
                "operationId": "op-write-1",
                "deadlineMs": 5000,
            }))
            .unwrap(),
        )
        .unwrap();

        let channel = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        channel.scan_and_ack_arms();

        // 1. Exact match returns true
        assert!(channel.matches_target_session(ATTACH_HANDSHAKE_BARRIER, "sess-target-42"));
        // 2. Mismatched session returns false
        assert!(!channel.matches_target_session(ATTACH_HANDSHAKE_BARRIER, "sess-other-99"));
        // 3. Reject None => true: missing targetBackendSessionId does NOT match any pane!
        assert!(!channel.matches_target_session(WRITE_BARRIER, "sess-target-42"));
        assert!(!channel.matches_target_session(WRITE_BARRIER, "any-pane"));

        // 4. Bound ACK emission
        channel
            .write_bound_ack(ATTACH_HANDSHAKE_BARRIER, "sess-target-42", "op-attach-1")
            .unwrap();
        let bound_ack_path = dir.join(format!("{ATTACH_HANDSHAKE_BARRIER}.bound-ack.json"));
        assert!(bound_ack_path.exists());
        let text = std::fs::read_to_string(&bound_ack_path).unwrap();
        let ack: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(ack["targetBackendSessionId"], json!("sess-target-42"));
        assert_eq!(ack["operationId"], json!("op-attach-1"));

        // 5. Worker queue seal & drain verification
        let ch_arc = std::sync::Arc::new(channel);
        ch_arc.schedule_cancellation_receipt(
            ATTACH_HANDSHAKE_BARRIER,
            "op-attach-1",
            json!({ "reason": "test" }),
        );
        let drain_res = ch_arc.drain_and_verify_workers().await;
        assert!(drain_res.is_ok());
        // Queue is sealed: subsequent calls execute synchronously without pushing new background tasks
        ch_arc.schedule_cancellation_receipt(
            ATTACH_HANDSHAKE_BARRIER,
            "op-attach-1",
            json!({ "reason": "post-seal" }),
        );
    }
}
