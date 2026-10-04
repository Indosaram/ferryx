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

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use serde_json::{json, Value};

use crate::ipc::debug::{classify_pane_liveness, PaneLivenessSnapshot, PaneLivenessVerdict};
use crate::native_terminal::surface_host::NativeTerminalSurfaceHostState;
use tauri::Manager;

pub const PRODUCER_ID: &str = "ipc-qa-barrier";
pub const WRITE_BARRIER: &str = "backend-write";
pub const PRESENTATION_BARRIER: &str = "presentation";
/// Runner contract: the operation nonce is handed to the product through the
/// private inherited env as well as through every arm file. Scenarios that
/// pre-arm no barrier (split-cancel, suspension-ownership, stale-binding) have
/// no arm to read it from, yet every emission must echo it or
/// `correlateReceipt` rejects the line.
pub const OPERATION_ID_ENV: &str = "FERRYX_QA_OPERATION_ID";
/// GUI-lane `fixture-setup` retry bound: the app's daemon attaches during
/// startup, so the first inventory read may race it. The wait is bounded and
/// the truth of the final attempt is always emitted.
const FIXTURE_BOOT_ATTEMPTS: u32 = 20;
const FIXTURE_BOOT_RETRY_MS: u64 = 250;

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

#[derive(Debug, Clone, serde::Deserialize)]
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
    #[serde(rename = "clientRequestId", default)]
    pub(crate) client_request_id: Option<String>,
    #[serde(rename = "sourceBackendSessionId", default)]
    pub(crate) source_backend_session_id: Option<String>,
    #[serde(rename = "workspaceId", default)]
    pub(crate) workspace_id: Option<String>,
    #[serde(rename = "worktreePath", default)]
    pub(crate) worktree_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaReservationSelector {
    pub run_id: String,
    pub operation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_backend_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_role: Option<String>,
}

impl QaReservationSelector {
    pub fn validate_target_role(&self) -> Result<(), &'static str> {
        if let Some(ref role) = self.target_role {
            if role == "predecessor" || role == "successor" {
                Ok(())
            } else {
                Err("targetRole must be 'predecessor' or 'successor', never a session ID or wildcard")
            }
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QaBoundAck {
    pub name: String,
    pub run_id: String,
    pub operation_id: String,
    pub producer: String,
    pub producer_pid: u32,
    pub target_backend_session_id: String,
    pub bound_at_ms: u64,
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
    pid: u32,
    arms: Mutex<HashMap<String, ArmSpec>>,
    /// The runner's operation nonce as handed over through the private env.
    /// Authoritative when present: a scenario that arms no barrier still needs
    /// a correlation identity for every receipt it settles.
    operation_nonce: Option<String>,
    /// Line counters per receipt file so appends stay one-JSON-per-line.
    receipt_lines: Mutex<HashMap<String, usize>>,
    held_tx: tokio::sync::broadcast::Sender<QaBarrierHeldEvent>,
    receipt_tx: tokio::sync::broadcast::Sender<QaBarrierReceiptEvent>,
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
        let (held_tx, _) = tokio::sync::broadcast::channel(64);
        let (receipt_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            dir,
            run_id,
            pid: std::process::id(),
            arms: Mutex::new(HashMap::new()),
            operation_nonce: None,
            receipt_lines: Mutex::new(HashMap::new()),
            held_tx,
            receipt_tx,
        }
    }

    /// Runner contract: the channel only exists through the private inherited
    /// env. Any other invocation (production/default runtime) is refused.
    pub fn from_env() -> Result<Self, String> {
        let dir = std::env::var("FERRYX_QA_BARRIER_DIR")
            .map_err(|_| "FERRYX_QA_BARRIER_DIR is not set".to_string())?;
        let run_id = std::env::var("FERRYX_QA_RUN_ID")
            .map_err(|_| "FERRYX_QA_RUN_ID is not set".to_string())?;
        let operation_nonce = std::env::var(OPERATION_ID_ENV).ok();
        Self::from_env_values(&dir, &run_id, operation_nonce.as_deref())
    }

    /// Pure core of [`Self::from_env`]: no process-global reads, so the boot
    /// decision is testable without mutating the shared environment.
    fn from_env_values(
        dir: &str,
        run_id: &str,
        operation_nonce: Option<&str>,
    ) -> Result<Self, String> {
        if dir.trim().is_empty() || run_id.trim().is_empty() {
            return Err("FERRYX_QA_BARRIER_DIR/FERRYX_QA_RUN_ID must be non-empty".into());
        }
        let operation_nonce = match operation_nonce {
            Some(value) if !value.trim().is_empty() => Some(value.to_string()),
            Some(_) => return Err(format!("{OPERATION_ID_ENV} must be non-empty when set")),
            None => None,
        };
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(format!("barrier dir does not exist: {}", dir.display()));
        }
        let mut channel = Self::new(dir, run_id.to_string());
        channel.operation_nonce = operation_nonce;
        Ok(channel)
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn producer_pid(&self) -> u32 {
        self.pid
    }

    /// Checks if an armed barrier targets a specific backend session ID.
    /// Explicitly rejects None => false: missing targetBackendSessionId does NOT match any pane!
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

    pub fn target_backend_session_id_for(&self, name: &str) -> Option<String> {
        self.arms
            .lock()
            .ok()?
            .get(name)
            .and_then(|s| s.target_backend_session_id.clone())
    }

    /// Checked live-arm binding:
    /// - targetRole must be predecessor or successor (never a session ID, no wildcard match)
    /// - identical duplicate allowed (returns existing bound ack)
    /// - conflict rejected (if already bound to different target_backend_session_id or different operation_id)
    /// - ACK only after installed
    pub fn bind_target_session(
        &self,
        name: &str,
        operation_id: &str,
        session_id: &str,
    ) -> Result<QaBoundAck, String> {
        if session_id.trim().is_empty() || session_id == "*" {
            return Err("target session id must be a concrete, non-wildcard session id".into());
        }
        let mut guard = self
            .arms
            .lock()
            .map_err(|_| "failed to lock barrier arms".to_string())?;
        let spec = guard
            .get_mut(name)
            .ok_or_else(|| format!("barrier '{name}' is not armed"))?;

        if let Some(ref role) = spec.target_role {
            if role != "predecessor" && role != "successor" {
                return Err("targetRole must be predecessor or successor only".into());
            }
        }

        if let Some(ref existing) = spec.target_backend_session_id {
            if existing == session_id && spec.operation_id == operation_id {
                // Identical duplicate allowed
                let ack = QaBoundAck {
                    name: name.to_string(),
                    run_id: self.run_id.clone(),
                    operation_id: operation_id.to_string(),
                    producer: PRODUCER_ID.to_string(),
                    producer_pid: self.pid,
                    target_backend_session_id: session_id.to_string(),
                    bound_at_ms: now_ms(),
                };
                return Ok(ack);
            }
            return Err(format!(
                "conflict: barrier '{name}' already bound to session '{existing}' (refusing '{session_id}')"
            ));
        }

        if spec.operation_id != operation_id {
            return Err(format!(
                "conflict: barrier operation mismatch: expected '{}', got '{operation_id}'",
                spec.operation_id
            ));
        }

        spec.target_backend_session_id = Some(session_id.to_string());

        let ack = QaBoundAck {
            name: name.to_string(),
            run_id: self.run_id.clone(),
            operation_id: operation_id.to_string(),
            producer: PRODUCER_ID.to_string(),
            producer_pid: self.pid,
            target_backend_session_id: session_id.to_string(),
            bound_at_ms: now_ms(),
        };

        // Write atomic bound-ack receipt file AFTER state is installed
        let ack_val = serde_json::to_value(&ack).map_err(|e| e.to_string())?;
        let filename = format!("{name}.bound-ack.json");
        write_json_atomic(&self.dir, &filename, &ack_val).map_err(|e| e.to_string())?;

        Ok(ack)
    }

    /// The runner stamps one operation nonce into every arm AND into the
    /// private env; emissions echo it. The env nonce is authoritative because a
    /// scenario that pre-arms no barrier has no arm to read it from.
    pub fn operation_id(&self) -> Option<String> {
        if let Some(nonce) = &self.operation_nonce {
            return Some(nonce.clone());
        }
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
            let ack = json!({
                "name": spec.name,
                "runId": spec.run_id,
                "operationId": spec.operation_id,
                "producer": PRODUCER_ID,
                "producerPid": self.pid,
                "registeredAtMs": now_ms(),
            });
            if write_json_atomic(&self.dir, &format!("{}.armed-ack.json", spec.name), &ack).is_ok()
            {
                if let Ok(mut arms) = self.arms.lock() {
                    arms.insert(spec.name.clone(), spec);
                }
                acked.push(name);
            }
        }
        (acked, rejected)
    }

    pub(crate) fn spec(&self, name: &str) -> Option<ArmSpec> {
        self.arms.lock().ok()?.get(name).cloned()
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

    pub(crate) fn write_held(
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
    pub(crate) fn append_receipt(&self, name: &str, operation_id: &str, settlement: Value) {
        let mut payload = json!({
            "runId": self.run_id,
            "operationId": operation_id,
            "producer": PRODUCER_ID,
            "producerPid": self.pid,
            "settledAtMs": now_ms(),
        });
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
        let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path)
        else {
            return;
        };
        if file.write_all(line.as_bytes()).is_ok() {
            let next_count = count + 1;
            if let Ok(mut lines) = self.receipt_lines.lock() {
                lines.insert(name.to_string(), next_count);
            }
            let _ = self.receipt_tx.send(QaBarrierReceiptEvent {
                name: name.to_string(),
                line_index: next_count,
                payload,
            });
        }
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

    /// `fixture-setup` settlement for the GUI lane: the real isolated-profile
    /// session inventory, one object per session as the runner's
    /// `validateFixtureSetup` requires. Returns false when the runner supplied
    /// no operation nonce, so a missing correlation is reported instead of
    /// writing an uncorrelatable line.
    pub fn emit_fixture_setup_from_sessions(&self, sessions: &[QaFixtureSession]) -> bool {
        let Some(operation_id) = self.operation_id() else {
            return false;
        };
        self.append_receipt(
            "fixture-setup",
            &operation_id,
            json!({
                "sessionId": sessions
                    .first()
                    .map(|session| session.backend_session_id.clone())
                    .unwrap_or_default(),
                "sessions": sessions,
                "fixtureKind": "gui-session-inventory",
            }),
        );
        true
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

/// Startup registration outcome for the GUI lane.
pub struct GuiBarrierBoot {
    pub channel: Arc<QaBarrierChannel>,
    pub acked: Vec<String>,
    pub rejected: Vec<String>,
}

/// GUI-boot installation of the private QA barrier channel.
///
/// The pane-liveness runner launches the real GUI with no arguments and hands
/// the channel to the process through the private inherited env only. Every
/// normal launch (no such env) returns `None` and installs nothing, so no QA
/// surface is reachable without the `local-split-qa` feature and the runner's
/// env. Installing here is what makes the already-implemented feature-gated
/// producers reachable on the GUI path: the real backend-write stage
/// (`ipc/native_terminal.rs`) and the real native presentation coordinator
/// (`native_terminal/surface_host.rs`) both consult `active_channel()`.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub fn install_for_gui_boot() -> Option<GuiBarrierBoot> {
    let channel = QaBarrierChannel::from_env().ok()?;
    install(channel);
    let channel = active_channel()?;
    let (acked, rejected) = channel.scan_and_ack_arms();
    Some(GuiBarrierBoot {
        channel,
        acked,
        rejected,
    })
}

/// One private fixture session observed in the GUI lane. Every field is a real
/// observation (the daemon's own session id / incarnation / epoch plus the
/// surface host's real collector snapshot); nothing is synthesized.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaFixtureSession {
    pub backend_session_id: String,
    pub kind: String,
    pub ownership_receipt: Value,
}

/// GUI-lane `fixture-setup` inventory.
///
/// The runner awaits `fixture-setup` line 0 immediately after launch, before
/// any trigger, so the GUI boot path reports the private fixture sessions that
/// really exist in the isolated profile. Only what the product can attest is
/// reported: `idle` when the surface host's real collector snapshot shows no
/// pending stage and no unpresented frame, `source` otherwise. The Task 3
/// snapshot kinds that require a fixture producer
/// (`created`/`adopted`/`externally-stopped`) are never claimed here - a
/// receipt that fabricated them would be simulated evidence.
pub async fn collect_gui_fixture_sessions(
    daemon_client: &crate::daemon::DaemonClient,
    surface_host: &NativeTerminalSurfaceHostState,
) -> Vec<QaFixtureSession> {
    let Ok(session_ids) = daemon_client.list_sessions().await else {
        return Vec::new();
    };
    let daemon_epoch = daemon_client.epoch();
    let mut sessions = Vec::new();
    for session_id in session_ids {
        let Ok(details) = daemon_client.describe_session(&session_id).await else {
            continue;
        };
        let idle = surface_host
            .session_liveness_observation(&session_id)
            .is_some_and(|snapshot| {
                snapshot.stage.is_none()
                    && snapshot.has_unpresented_frames == Some(false)
                    && classify_pane_liveness(&snapshot) == PaneLivenessVerdict::Idle
            });
        sessions.push(QaFixtureSession {
            backend_session_id: session_id.clone(),
            kind: if idle {
                "idle".to_string()
            } else {
                "source".to_string()
            },
            ownership_receipt: json!({
                "backendSessionId": session_id,
                "incarnation": details.incarnation,
                "daemonEpoch": daemon_epoch.map(|epoch| epoch.to_string()),
                "running": details.running,
                "workspaceId": details.workspace_id,
                "cwd": details.cwd,
            }),
        });
    }
    sessions
}

/// Outcome of the GUI-lane `fixture-setup` settlement.
pub struct GuiFixtureSetupOutcome {
    pub emitted: bool,
    pub sessions: Vec<QaFixtureSession>,
}

/// GUI-boot entry point. Installs the channel when the runner handed the
/// process one, then settles `fixture-setup` from the real isolated-profile
/// session inventory off the main thread. Returns immediately; a launch without
/// the runner env does nothing at all.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub fn start_gui_boot_channel<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    daemon_client: Arc<crate::daemon::DaemonClient>,
) {
    let Some(boot) = install_for_gui_boot() else {
        return;
    };
    if !boot.rejected.is_empty() {
        eprintln!(
            "FERRYX_QA_ARM_REJECTED: rejected prelaunch arms: {:?}",
            boot.rejected
        );
    }
    tauri::async_runtime::spawn(async move {
        let outcome = emit_gui_fixture_setup(&boot.channel, &daemon_client, &app).await;
        if !outcome.emitted {
            eprintln!(
                "FERRYX_QA_FIXTURE_SETUP_UNSETTLED: {} real session(s) observed but no operation nonce was supplied",
                outcome.sessions.len()
            );
        }
    });
}

/// Bounded GUI-lane `fixture-setup`: the app's daemon attaches during startup,
/// so the first inventory read may race it. The wait is bounded and the truth
/// of the final attempt is always emitted.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub async fn emit_gui_fixture_setup<R: tauri::Runtime>(
    channel: &QaBarrierChannel,
    daemon_client: &crate::daemon::DaemonClient,
    app: &tauri::AppHandle<R>,
) -> GuiFixtureSetupOutcome {
    let mut sessions = Vec::new();
    for attempt in 0..FIXTURE_BOOT_ATTEMPTS {
        sessions = collect_gui_fixture_sessions(
            daemon_client,
            &app.state::<NativeTerminalSurfaceHostState>(),
        )
        .await;
        if !sessions.is_empty() || attempt + 1 == FIXTURE_BOOT_ATTEMPTS {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(FIXTURE_BOOT_RETRY_MS)).await;
    }
    let emitted = channel.emit_fixture_setup_from_sessions(&sessions);
    GuiFixtureSetupOutcome { emitted, sessions }
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
    let write_session = "qa-headless-write";
    let presentation_session = "qa-headless-presentation";
    channel.emit_fixture_setup(&[write_session, presentation_session]);

    match run_headless_stages(&channel, write_session, presentation_session) {
        Ok(summary) => {
            deactivate();
            println!("{}", serde_json::to_string_pretty(&summary).unwrap_or_default());
            0
        }
        Err(error) => {
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
        crate::ipc::native_terminal::send_native_terminal_input_with_stage_logging(
            &app.handle(),
            &state,
            write_session,
            &input,
            Some(1),
            Some(operation_id),
            |_bytes| async { Ok(()) },
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

    // The GUI boot path must install nothing unless the runner handed the
    // process a private barrier dir through the inherited env. `from_env_values`
    // is the pure core of that decision, so no test mutates the shared process
    // environment (and cannot race a neighbouring test).
    #[test]
    fn gui_boot_install_requires_the_private_runner_env() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap();

        // No barrier dir: no channel, therefore no QA surface at all.
        assert!(QaBarrierChannel::from_env_values("", TEST_RUN_ID, None).is_err());
        assert!(QaBarrierChannel::from_env_values(dir_str, "", None).is_err());
        // A dir the runner never created is refused rather than installed.
        assert!(QaBarrierChannel::from_env_values("/definitely/not/a/barrier/dir", TEST_RUN_ID, None).is_err());
        // A non-empty operation nonce is mandatory once the key is present.
        assert!(QaBarrierChannel::from_env_values(dir_str, TEST_RUN_ID, Some("   ")).is_err());

        // Runner env present: installed, and the env nonce is the correlation
        // identity for a scenario that pre-arms no barrier.
        let channel = QaBarrierChannel::from_env_values(dir_str, TEST_RUN_ID, Some(TEST_OPERATION_ID))
            .expect("runner env installs the channel");
        assert_eq!(channel.run_id(), TEST_RUN_ID);
        assert_eq!(channel.operation_id(), Some(TEST_OPERATION_ID.to_string()));
        assert_eq!(channel.producer_pid(), std::process::id());

        // Headless path unchanged: the arm's nonce still resolves when the env
        // carries none.
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let from_arm = QaBarrierChannel::from_env_values(dir_str, TEST_RUN_ID, None)
            .expect("runner env without the optional nonce still installs");
        from_arm.scan_and_ack_arms();
        assert_eq!(from_arm.operation_id(), Some(TEST_OPERATION_ID.to_string()));
    }

    // GUI-lane fixture inventory: only the sessions that really exist are
    // reported, and a scenario with no barrier still settles a correlatable
    // line because the env nonce is authoritative.
    #[test]
    fn gui_fixture_setup_reports_real_sessions_only() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        // No arm is written: this is the split-cancel / suspension-ownership /
        // stale-binding shape, where only the env nonce can correlate.
        let channel = QaBarrierChannel::from_env_values(
            dir.to_str().unwrap(),
            TEST_RUN_ID,
            Some(TEST_OPERATION_ID),
        )
        .unwrap();

        // Zero observed sessions must still settle truthfully: an empty
        // inventory, never an invented fixture.
        assert!(channel.emit_fixture_setup_from_sessions(&[]));
        let empty = read_lines(&dir, "fixture-setup");
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0]["sessions"], json!([]));
        assert_eq!(empty[0]["runId"], json!(TEST_RUN_ID));
        assert_eq!(empty[0]["operationId"], json!(TEST_OPERATION_ID));
        assert_eq!(empty[0]["producer"], json!(PRODUCER_ID));

        let observed = vec![
            QaFixtureSession {
                backend_session_id: "pty-real-1".to_string(),
                kind: "idle".to_string(),
                ownership_receipt: json!({"backendSessionId": "pty-real-1", "daemonEpoch": "42"}),
            },
            QaFixtureSession {
                backend_session_id: "pty-real-2".to_string(),
                kind: "source".to_string(),
                ownership_receipt: json!({"backendSessionId": "pty-real-2", "incarnation": "inc-2"}),
            },
        ];
        assert!(channel.emit_fixture_setup_from_sessions(&observed));
        let lines = read_lines(&dir, "fixture-setup");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["sessionId"], json!("pty-real-1"));
        assert_eq!(lines[1]["fixtureKind"], json!("gui-session-inventory"));
        assert_eq!(lines[1]["sessions"][0]["backendSessionId"], json!("pty-real-1"));
        assert_eq!(lines[1]["sessions"][0]["kind"], json!("idle"));
        assert_eq!(lines[1]["sessions"][1]["ownershipReceipt"]["incarnation"], json!("inc-2"));
        // The kinds the product cannot attest are never claimed.
        for line in &lines {
            for session in line["sessions"].as_array().unwrap() {
                let kind = session["kind"].as_str().unwrap();
                assert!(
                    kind == "idle" || kind == "source",
                    "GUI inventory must not fabricate fixture kinds: {kind}"
                );
            }
        }

        // Without a correlation identity the line is refused instead of
        // written uncorrelatable.
        let uncorrelated = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        assert!(!uncorrelated.emit_fixture_setup_from_sessions(&observed));
        assert_eq!(read_lines(&dir, "fixture-setup").len(), 2);
    }
}
