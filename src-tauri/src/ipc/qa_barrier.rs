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
/// Runner contract: the fixture kinds the GUI lane must CONSTRUCT before it
/// settles `fixture-setup`, as a comma-separated list handed over in the same
/// private env as the channel itself. Absent or empty constructs nothing, so a
/// normal launch - and a runner that predates the key - reaches no creation path
/// at all.
pub const FIXTURE_KINDS_ENV: &str = "FERRYX_QA_FIXTURE_KINDS";

/// The slice of the runner's `fixture-setup` budget this boot spends building
/// fixtures.
///
/// The runner awaits receipt line 0 with `BUDGETS.stagePrepareCreateStatusMs`
/// (9 s, `scripts/lib/qa-scenarios/common-harness.mjs`) measured from the launch
/// it performs, and this boot starts seconds into that window - the pass-2 probe
/// measured a settlement at T+10 s against the same 9 s budget. Creation is
/// therefore bounded by a deadline on the daemon's own replies (never a fixed
/// sleep): a fixture that cannot be built inside it is reported unbuilt with the
/// daemon's own error, and the settlement still lands inside the runner's budget.
const FIXTURE_CREATE_BUDGET_MS: u64 = 3_500;

/// The private fixture workspace, created inside the runner's own barrier dir so
/// the runner's isolation-root cleanup removes it with the rest of the run.
const FIXTURE_WORKSPACE_DIR: &str = "fixture-workspace";

/// The workspace id the fixture sessions are registered under in the daemon: the
/// daemon refuses a spawn in a workspace it does not know, exactly as it does for
/// the GUI's own spawn.
const FIXTURE_WORKSPACE_ID: &str = "qa-fixture";

/// Fixture geometry: the daemon's default shell at a warm size.
const FIXTURE_COLS: u16 = 80;
const FIXTURE_ROWS: u16 = 24;

/// One fixture kind the runner can ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QaFixtureKind {
    /// A session that exists and is running.
    Source,
    /// A session this QA boot really created through the local-split create path.
    Created,
    /// A session the daemon reports running and unblocked.
    Idle,
    /// A genuinely stopped, unowned session.
    ExternallyStopped,
    /// A session adopted through a real retained handover.
    Adopted,
}

impl QaFixtureKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "source" => Some(Self::Source),
            "created" => Some(Self::Created),
            "idle" => Some(Self::Idle),
            "externally-stopped" => Some(Self::ExternallyStopped),
            "adopted" => Some(Self::Adopted),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Created => "created",
            Self::Idle => "idle",
            Self::ExternallyStopped => "externally-stopped",
            Self::Adopted => "adopted",
        }
    }

    /// The real construction path this lane can drive for the kind, or the exact
    /// hook the kind needs instead of a session this lane cannot honestly build.
    pub fn creation(self) -> FixtureCreation {
        match self {
            Self::Source | Self::Idle => FixtureCreation::PlainSpawn,
            Self::Created => FixtureCreation::LocalSplit,
            // A stopped-but-unowned session is a plain spawn whose process is then
            // really stopped through the PTY signal path. Measured: that path sends
            // a raw `SIGSTOP` (`terminal/session.rs` `TerminalSignal::Stop =>
            // libc::SIGSTOP`) and never touches `suspension.rs::ownership()`, so the
            // suspension ledger holds no entry and `classify_stop_source` reports
            // `External` - the honest classification for a stop it did not actuate.
            Self::ExternallyStopped => FixtureCreation::StoppedPlainSpawn,
            // An adopted session needs a real retained handover: the
            // predecessor-export/successor-adopt producers over daemon/handover.rs.
            Self::Adopted => FixtureCreation::Unsupported(
                "needs a real retained handover (predecessor-export/successor-adopt producers in daemon/qa_producers.rs over daemon/handover.rs); out of scope for fixture construction",
            ),
        }
    }
}

/// How this lane can really construct one fixture kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureCreation {
    /// The daemon's plain spawn (`DaemonRequest::Spawn`), the request the GUI
    /// issues when it opens a terminal.
    PlainSpawn,
    /// The local-split create path (`prepare_local_split_until` ->
    /// `create_local_split_until`), the path the GUI's create-only branch uses.
    LocalSplit,
    /// The daemon's plain spawn, followed by an external stop of the fixture's
    /// process through the PTY signal path (`TerminalSignal::Stop` -> `SIGSTOP`).
    /// That path deliberately does NOT go through the suspension ownership ledger
    /// (`terminal/suspension.rs::ownership()`), so the resulting stop is exactly
    /// what this kind means: a real, kernel-observed stop that the suspension
    /// subsystem never actuated and must therefore never claim or auto-resume.
    StoppedPlainSpawn,
    /// Not constructible from this lane: the exact hook the kind needs.
    Unsupported(&'static str),
}

/// Parses the runner's comma-separated fixture-kind list.
///
/// An unknown name is refused rather than dropped: a runner typo must fail loudly
/// instead of producing a half-built fixture that then reads as a product defect.
/// Duplicates collapse, because one session per kind is what the runner's fixture
/// validator asks for.
pub fn parse_fixture_kinds(raw: Option<&str>) -> Result<Vec<QaFixtureKind>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let mut kinds = Vec::new();
    for name in raw.split(',') {
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let kind = QaFixtureKind::parse(name).ok_or_else(|| format!("unknown fixture kind '{name}'"))?;
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    Ok(kinds)
}

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

/// The GUI lane's own daemon client, installed by the QA boot path.
///
/// The native render path holds no daemon client of its own, yet a positive
/// recovery verdict needs the daemon's OWN reader/kernel facts. This handle is
/// that client: the settlements observe the session through it instead of
/// inventing values. `None` in every normal launch and in the headless lane,
/// which runs without a daemon.
static QA_DAEMON_CLIENT: RwLock<Option<Arc<crate::daemon::DaemonClient>>> = RwLock::new(None);

pub fn install_daemon_client(client: Arc<crate::daemon::DaemonClient>) {
    if let Ok(mut guard) = QA_DAEMON_CLIENT.write() {
        *guard = Some(client);
    }
}

pub fn qa_daemon_client() -> Option<Arc<crate::daemon::DaemonClient>> {
    QA_DAEMON_CLIENT.read().ok().and_then(|guard| guard.clone())
}

/// The daemon's own liveness facts for one session, as THIS process observed
/// them. Every field stays `None` unless the daemon really reported it: the
/// classifier reads `false` as "verified not blocked", so an unobserved fact must
/// never be defaulted to `false`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QaDaemonLiveness {
    /// True only when the daemon answered a real describe for the session.
    pub observed: bool,
    pub reader_paused: Option<bool>,
    pub kernel_stopped: Option<bool>,
    pub suspended: Option<bool>,
    pub daemon_epoch: Option<String>,
    /// Why the facts could not be observed, when they could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

impl QaDaemonLiveness {
    /// Applies the observed facts to a snapshot. Unobserved fields are left
    /// exactly as they were (`None`), so the classifier keeps answering `Unknown`
    /// with `evidenceMissing` instead of a fabricated recovery.
    pub fn apply(&self, snapshot: &mut PaneLivenessSnapshot) {
        if let Some(value) = self.reader_paused {
            snapshot.reader_paused = Some(value);
        }
        if let Some(value) = self.kernel_stopped {
            snapshot.kernel_stopped = Some(value);
        }
        if let Some(value) = self.suspended {
            snapshot.suspended = Some(value);
        }
        if let Some(ref epoch) = self.daemon_epoch {
            snapshot.daemon_epoch = Some(epoch.clone());
        }
    }
}

/// Pure mapping of one real describe reply. `DaemonSessionDetails::suspended` is
/// a plain `bool` the daemon always answers, so it is a real observation;
/// `reader_paused`/`kernel_stopped` stay `None` on a daemon that does not report
/// them (they are `Option` on the wire for exactly that reason), which keeps the
/// verdict honest on older daemons instead of turning a missing observation into
/// a positive recovery.
pub fn daemon_liveness_from_details(
    details: &crate::daemon::protocol::DaemonSessionDetails,
    daemon_epoch: Option<u64>,
) -> QaDaemonLiveness {
    QaDaemonLiveness {
        observed: true,
        reader_paused: details.reader_paused,
        kernel_stopped: details.kernel_stopped,
        suspended: Some(details.suspended),
        daemon_epoch: daemon_epoch.map(|epoch| epoch.to_string()),
        failure: None,
    }
}

/// Asks the daemon that really owns the session. A refused or failed request
/// yields `observed: false` with the reason and every fact `None` - never a
/// defaulted value.
pub async fn observe_daemon_liveness(
    client: &crate::daemon::DaemonClient,
    session_id: &str,
) -> QaDaemonLiveness {
    match client.describe_session(session_id).await {
        Ok(details) => daemon_liveness_from_details(&details, client.epoch()),
        Err(error) => QaDaemonLiveness {
            observed: false,
            failure: Some(format!("{error:?}")),
            ..Default::default()
        },
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

    /// The private control directory this channel was built from. Producers used
    /// to re-read `FERRYX_QA_BARRIER_DIR` because the channel exposed no path
    /// accessor; this is the same value the channel itself reads and writes.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// Reads one runner->product command (`<name>.request.json`) for THIS run.
    ///
    /// The runner writes commands after launch, so no producer can read them at
    /// boot. A control that does not echo this channel's run AND operation nonces
    /// is never returned - exactly like a mismatched release control - so a stale
    /// or replayed command from another run cannot drive the product. The file is
    /// left untouched: the caller decides when a command was really serviced.
    pub fn take_command(&self, name: &str) -> Option<Value> {
        self.correlated_control(name, "request.json")
    }

    /// The runner's live-arm binding for one barrier (`<name>.bind.json`), read
    /// through the same nonce check as every other control. The runner binds a
    /// barrier to the session it really created (`bindBackendSession`), which no
    /// product observation can know in advance.
    pub fn take_bind(&self, name: &str) -> Option<Value> {
        self.correlated_control(name, "bind.json")
    }

    /// One control file, accepted only when it echoes this run's nonces.
    fn correlated_control(&self, name: &str, suffix: &str) -> Option<Value> {
        let text = std::fs::read_to_string(self.dir.join(format!("{name}.{suffix}"))).ok()?;
        let value: Value = serde_json::from_str(&text).ok()?;
        if value.get("runId").and_then(Value::as_str) != Some(self.run_id.as_str()) {
            return None;
        }
        let expected = self.operation_id()?;
        if value.get("operationId").and_then(Value::as_str) != Some(expected.as_str()) {
            return None;
        }
        Some(value)
    }

    /// Adopts the runner's live-arm binding for one barrier through the channel's
    /// own checked binding rule (concrete non-wildcard session, no conflict with an
    /// already bound target) and returns the bound session id.
    ///
    /// The installed target is never overwritten: a bind that conflicts with a
    /// target the product already bound is refused and reported, so a control file
    /// cannot redirect a barrier away from the session the product really owns.
    pub fn adopt_runner_bind(&self, name: &str) -> Result<String, String> {
        let bind = self
            .take_bind(name)
            .ok_or_else(|| format!("no correlated {name}.bind.json for this run"))?;
        let target = bind
            .get("targetBackendSessionId")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let operation_id = self
            .operation_id()
            .ok_or_else(|| "no operation nonce for the runner bind".to_string())?;
        self.bind_target_session(name, &operation_id, &target)?;
        Ok(target)
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
                    // The rejected-control record is a synchronous write; it runs on the
                    // blocking pool so this poll cadence never stalls a runtime worker.
                    let rejected_dir = self.dir.clone();
                    let rejected_file = format!("{}.rejected-control.json", spec.name);
                    let rejected_value = json!({
                        "runId": self.run_id,
                        "operationId": spec.operation_id,
                        "producer": PRODUCER_ID,
                        "producerPid": self.pid,
                        "rejectedAtMs": now_ms(),
                        "reason": "run-or-operation-identity-mismatch",
                    });
                    let _ = crate::ipc::run_blocking(move || {
                        Ok(write_json_atomic(&rejected_dir, &rejected_file, &rejected_value))
                    })
                    .await;
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
    /// `validateFixtureSetup` requires, plus the audit of what this boot really
    /// created and what it could not. Returns false when the runner supplied no
    /// operation nonce, so a missing correlation is reported instead of writing
    /// an uncorrelatable line.
    pub fn emit_fixture_setup_from_sessions(
        &self,
        inventory: &GuiFixtureInventory,
        creation: &GuiFixtureCreation,
    ) -> bool {
        let Some(operation_id) = self.operation_id() else {
            return false;
        };
        let sessions = &inventory.sessions;
        let mut claimed: Vec<&str> = Vec::new();
        for session in sessions {
            if !claimed.contains(&session.kind.as_str()) {
                claimed.push(session.kind.as_str());
            }
        }
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
                // The audit of the construction attempt: what was asked for, what
                // was really built, what was refused, and how long it took. A kind
                // that could not be built is reported here instead of being
                // silently absent from `sessions`.
                "fixtureKindsRequested": creation
                    .requested
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>(),
                "fixtureKindsClaimed": claimed,
                "fixtureKindsUnsupported": creation
                    .unsupported
                    .iter()
                    .map(|(kind, hook)| json!({ "kind": kind.as_str(), "hook": hook }))
                    .collect::<Vec<_>>(),
                "fixtureCreationFailures": creation.failures,
                "fixtureCreationElapsedMs": creation.elapsed_ms,
                "fixtureClaimsRefused": inventory.refused,
                "sessionsNotRunning": inventory.not_running,
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
    /// The kernel probe the `externally-stopped` kind was classified from, and
    /// only then: the runner's fixture validator requires `stopped` evidence on
    /// that kind, so an idle/source session must not carry a probe state it does
    /// not have.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_probe_state: Option<String>,
    pub ownership_receipt: Value,
}

/// The surface host's own per-session observation, read through a handle a
/// spawned future may safely hold.
///
/// `NativeTerminalSurfaceHostState` owns the WGPU worker, so a future that holds
/// Tauri's `State<'_, NativeTerminalSurfaceHostState>` - or a plain
/// `&NativeTerminalSurfaceHostState` - across an `await` forces the compiler to
/// prove that managed state's `Sync` through the whole WGPU object graph
/// (`NumericDimension` -> `InterfaceVar` -> ... -> `ResourceState`). That
/// derivation is deep enough to overflow the default recursion limit, which fails
/// `cargo build --features local-split-qa` with `E0275` while `cargo check` passes.
///
/// Every implementation resolves the state inside one synchronous call and returns
/// only the owned snapshot, so no future's `Send` proof ever mentions the managed
/// state itself.
pub trait SurfaceHostObservations {
    /// The host's own observation for the session, or `None` when this process
    /// holds none - the honest "not observed" the classifier requires instead of a
    /// defaulted `false`.
    fn observe_session(&self, session_id: &str) -> Option<PaneLivenessSnapshot>;
}

/// The GUI boot lane holds no host reference: it resolves the managed state on
/// demand, inside a call that contains no `await`.
impl<R: tauri::Runtime> SurfaceHostObservations for tauri::AppHandle<R> {
    fn observe_session(&self, session_id: &str) -> Option<PaneLivenessSnapshot> {
        // One statement, no `await`: the `State` borrow ends with it.
        self.state::<NativeTerminalSurfaceHostState>()
            .session_liveness_observation(session_id)
    }
}

/// Lanes that already hold the host read it directly (tests, in-process
/// fixtures). Nothing in those lanes is spawned, so no auto-trait proof is
/// involved; the impl is test-only so no spawned lane can reach for it.
#[cfg(test)]
impl SurfaceHostObservations for &NativeTerminalSurfaceHostState {
    fn observe_session(&self, session_id: &str) -> Option<PaneLivenessSnapshot> {
        self.session_liveness_observation(session_id)
    }
}

/// GUI-lane `fixture-setup` inventory.
///
/// The runner awaits `fixture-setup` line 0 immediately after launch, before any
/// trigger, so the GUI boot path reports the private fixture sessions that really
/// exist in the isolated profile. Every claim is made from the daemon's own reply
/// about that session (plus the surface host's observation when this process has
/// one), never from the request: `externally-stopped` when the kernel reports the
/// process stopped and the daemon's own lifecycle registry does not own that stop,
/// `idle` when the daemon reports the session running with its reader unpaused,
/// its process not kernel-stopped and not suspended, `source` otherwise. The kind
/// this boot really created through the local-split create path is claimed as
/// `created`. A kind the daemon's reply does not support is refused and reported,
/// and a session the daemon does not report running is listed but never claimed.
///
/// The host is read through [`SurfaceHostObservations`] rather than as a
/// `&NativeTerminalSurfaceHostState`: the GUI boot lane awaits this collector
/// inside a spawned future, and holding the managed-state borrow across those
/// awaits would demand the `Sync` proof described on that trait.
pub async fn collect_gui_fixture_sessions<H: SurfaceHostObservations>(
    daemon_client: &crate::daemon::DaemonClient,
    surface_host: H,
    creation: &GuiFixtureCreation,
) -> GuiFixtureInventory {
    let mut inventory = GuiFixtureInventory::default();
    let Ok(session_ids) = daemon_client.list_sessions().await else {
        return inventory;
    };
    let daemon_epoch = daemon_client.epoch();
    for session_id in session_ids {
        let Ok(details) = daemon_client.describe_session(&session_id).await else {
            continue;
        };
        // A fixture has to be a session that really runs: a listed but exited
        // session is observed and reported, never claimed as a usable fixture.
        if !details.running {
            inventory.not_running.push(session_id);
            continue;
        }
        let observed = surface_host.observe_session(&session_id);
        let intended = creation.assigned_kind(&session_id);
        let classification = classify_fixture_session(&details, observed.as_ref(), intended);
        if let Some(refusal) = classification.refusal {
            inventory.refused.push(format!(
                "{session_id}: intended {} refused: {refusal}",
                intended.map(QaFixtureKind::as_str).unwrap_or("observed-kind")
            ));
        }
        inventory.sessions.push(QaFixtureSession {
            backend_session_id: session_id.clone(),
            kind: classification.kind.to_string(),
            stop_probe_state: classification.stop_probe_state.map(str::to_string),
            ownership_receipt: json!({
                "backendSessionId": session_id,
                "incarnation": details.incarnation,
                "daemonEpoch": daemon_epoch.map(|epoch| epoch.to_string()),
                "running": details.running,
                "workspaceId": details.workspace_id,
                "cwd": details.cwd,
                // The real probes the classification was made from.
                "readerPaused": details.reader_paused,
                "kernelStopped": details.kernel_stopped,
                "suspended": details.suspended,
                "registrySuspended": details.registry_suspended,
                "suspensionSource": details.suspension_source,
                // The exact evidence the kind claim rests on, so the claim is
                // auditable instead of asserted.
                "fixtureKindBasis": classification.basis.as_str(),
                "qaCreated": intended.is_some(),
            }),
        });
    }
    inventory
}

/// The real isolated-profile inventory one `fixture-setup` settlement reports.
#[derive(Debug, Clone, Default)]
pub struct GuiFixtureInventory {
    pub sessions: Vec<QaFixtureSession>,
    /// Intended kinds the daemon's own reply refused to support, named with the
    /// missing evidence: a refused claim is reported, never silently downgraded
    /// and never replaced by an invented session.
    pub refused: Vec<String>,
    /// Listed sessions the daemon does not report running: observed, never claimed
    /// as a fixture.
    pub not_running: Vec<String>,
}

impl GuiFixtureInventory {
    /// An observed inventory with no claim refusals, for callers that have no
    /// creation step (and for the settlement tests).
    pub fn from_sessions(sessions: Vec<QaFixtureSession>) -> Self {
        Self {
            sessions,
            ..Default::default()
        }
    }
}

/// The evidence one fixture kind claim rests on. Every variant names a fact this
/// process really observed; there is no "assumed" basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKindBasis {
    /// The daemon reports the session running.
    DaemonRunning,
    /// This QA boot created the session through the real local-split create path.
    QaLocalSplitCreate,
    /// The daemon reports the session running, its reader unpaused, its process
    /// not kernel-stopped and not suspended.
    DaemonIdleFacts,
    /// The same facts, plus the surface host's own observation of no pending
    /// render for this session.
    DaemonIdleFactsAndSurfaceHost,
    /// The kernel reports the process stopped and the daemon's own lifecycle
    /// registry does not own that stop.
    KernelStopUnowned,
}

impl FixtureKindBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DaemonRunning => "daemon-reports-running",
            Self::QaLocalSplitCreate => "qa-local-split-create",
            Self::DaemonIdleFacts => "daemon-idle-facts",
            Self::DaemonIdleFactsAndSurfaceHost => "daemon-idle-facts+surface-host",
            Self::KernelStopUnowned => "kernel-stop-unowned",
        }
    }
}

/// One running session's honest fixture classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureClassification {
    pub kind: &'static str,
    pub basis: FixtureKindBasis,
    /// Set only for `externally-stopped`: the runner's validator requires
    /// `stopped` evidence on that kind, so no other kind may carry a probe state.
    pub stop_probe_state: Option<&'static str>,
    /// Set when an intended kind had to be refused, naming the missing evidence.
    pub refusal: Option<&'static str>,
}

/// Classifies one running session for `fixture-setup`.
///
/// `intended` is the kind this QA boot really established for the session (the
/// local-split create reply for `created`, the spawn reply for `source`/`idle`).
/// It is provenance, not evidence: an observation the daemon really reported
/// outranks it, and an intention the daemon's reply does not support is refused
/// and reported instead of being downgraded silently.
fn classify_fixture_session(
    details: &crate::daemon::protocol::DaemonSessionDetails,
    observed: Option<&PaneLivenessSnapshot>,
    intended: Option<QaFixtureKind>,
) -> FixtureClassification {
    let observation = observation_classification(details, observed);
    let Some(intended) = intended else {
        return observation;
    };
    // A real external stop is an observation: it is reported whatever kind was
    // asked for, and it carries the stop probe the runner's validator requires.
    if observation.kind == "externally-stopped" {
        return observation;
    }
    match intended {
        QaFixtureKind::Created => FixtureClassification {
            kind: "created",
            basis: FixtureKindBasis::QaLocalSplitCreate,
            stop_probe_state: None,
            refusal: None,
        },
        QaFixtureKind::Source => FixtureClassification {
            kind: "source",
            basis: FixtureKindBasis::DaemonRunning,
            stop_probe_state: None,
            refusal: None,
        },
        QaFixtureKind::Idle => match idle_basis(details, observed) {
            Ok(basis) => FixtureClassification {
                kind: "idle",
                basis,
                stop_probe_state: None,
                refusal: None,
            },
            Err(refusal) => FixtureClassification {
                refusal: Some(refusal),
                ..observation
            },
        },
        // `adopted` needs a real retained handover (a second daemon adopting this
        // one's sessions over `daemon/handover.rs`), which is not a session this
        // lane can build, so the claim is refused with that reason instead of being
        // downgraded. `externally-stopped` is deliberately absent from this arm: it
        // is constructible now, and a real external stop is already returned as an
        // observation above.
        QaFixtureKind::Adopted => FixtureClassification {
            refusal: Some("kind-not-constructible-in-this-lane"),
            ..observation
        },
        // Constructible, but it can only be CLAIMED from the observation above: a
        // session whose kernel stop the daemon does not attribute to itself is the
        // kind, and the classification for one that is still running is whatever the
        // daemon's own reply describes it as. Reaching here means the fixture's stop
        // was not observed, so the honest answer is the observation - never a claim
        // that the session is stopped when the daemon reports otherwise.
        QaFixtureKind::ExternallyStopped => observation,
    }
}

/// The kind an unclaimed session is honestly observed as.
fn observation_classification(
    details: &crate::daemon::protocol::DaemonSessionDetails,
    observed: Option<&PaneLivenessSnapshot>,
) -> FixtureClassification {
    if external_stop_basis(details).is_some() {
        return FixtureClassification {
            kind: "externally-stopped",
            basis: FixtureKindBasis::KernelStopUnowned,
            stop_probe_state: Some("stopped"),
            refusal: None,
        };
    }
    match idle_basis(details, observed) {
        Ok(basis) => FixtureClassification {
            kind: "idle",
            basis,
            stop_probe_state: None,
            refusal: None,
        },
        Err(_) => FixtureClassification {
            kind: "source",
            basis: FixtureKindBasis::DaemonRunning,
            stop_probe_state: None,
            refusal: None,
        },
    }
}

/// `externally-stopped` is an OBSERVATION, never a guess: the kernel must report
/// the process stopped (`kernelStopped`) AND the daemon must report that its own
/// lifecycle registry does not own the stop (`registrySuspended == Some(false)`,
/// or the explicit `external-kernel` attribution). A stopped session whose
/// ownership the daemon does not report at all stays `source`: this lane cannot
/// tell an external stop from a Ferryx-owned one, and claiming the kind would be a
/// fabricated fixture.
fn external_stop_basis(
    details: &crate::daemon::protocol::DaemonSessionDetails,
) -> Option<FixtureKindBasis> {
    let stopped = details.kernel_stopped == Some(true);
    let unowned = details.registry_suspended == Some(false)
        || details.suspension_source.as_deref() == Some("external-kernel");
    (stopped && unowned).then_some(FixtureKindBasis::KernelStopUnowned)
}

/// The evidence an `idle` claim may rest on, or the reason it may not be made.
///
/// The daemon's own reader/kernel/suspension facts are required in full: an
/// unobserved field is never read as "not blocked", because the classifier reads
/// a defaulted `false` as a verified fact. When this process also holds the
/// surface host's observation for the session, that observation vetoes the claim
/// if it contradicts it.
fn idle_basis(
    details: &crate::daemon::protocol::DaemonSessionDetails,
    observed: Option<&PaneLivenessSnapshot>,
) -> Result<FixtureKindBasis, &'static str> {
    if details.reader_paused.is_none() || details.kernel_stopped.is_none() {
        return Err("daemon-did-not-report-reader-and-kernel-facts");
    }
    if details.reader_paused == Some(true) {
        return Err("daemon-reports-reader-paused");
    }
    if details.kernel_stopped == Some(true) {
        return Err("daemon-reports-process-kernel-stopped");
    }
    if details.suspended {
        return Err("daemon-reports-session-suspended");
    }
    match observed {
        None => Ok(FixtureKindBasis::DaemonIdleFacts),
        Some(snapshot) => {
            if snapshot.stage.is_some() {
                return Err("surface-host-observes-an-active-stage");
            }
            if snapshot.has_unpresented_frames != Some(false) {
                return Err("surface-host-observes-unpresented-frames");
            }
            Ok(FixtureKindBasis::DaemonIdleFactsAndSurfaceHost)
        }
    }
}

/// What the QA boot really established for the fixture, and what it could not.
///
/// `assignments` is provenance, not evidence: the settlement still re-reads the
/// daemon's own reply about each session and refuses any claim that reply does
/// not support.
#[derive(Debug, Clone, Default)]
pub struct GuiFixtureCreation {
    /// The kinds the runner asked for (`FERRYX_QA_FIXTURE_KINDS`).
    pub requested: Vec<QaFixtureKind>,
    /// Session id -> the kind this boot really created it as.
    pub assignments: Vec<(String, QaFixtureKind)>,
    /// Kinds this lane cannot construct, each with the exact hook it needs.
    pub unsupported: Vec<(QaFixtureKind, &'static str)>,
    /// Real creation failures, with the daemon's own error.
    pub failures: Vec<String>,
    /// Wall time the creation step spent, so the runner's budget question is
    /// answered with a measurement instead of an estimate.
    pub elapsed_ms: u64,
}

impl GuiFixtureCreation {
    /// The kind this boot really created the session as, if it created it.
    pub fn assigned_kind(&self, session_id: &str) -> Option<QaFixtureKind> {
        self.assignments
            .iter()
            .find(|(id, _)| id == session_id)
            .map(|(_, kind)| *kind)
    }
}

/// Outcome of the GUI-lane `fixture-setup` settlement.
pub struct GuiFixtureSetupOutcome {
    pub emitted: bool,
    pub sessions: Vec<QaFixtureSession>,
    pub creation: GuiFixtureCreation,
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
    // The GUI lane's own daemon client, kept for the settlements that must report
    // the daemon's reader/kernel facts instead of inventing them.
    install_daemon_client(Arc::clone(&daemon_client));
    // The runner's fixture request travels in the same private env as the channel
    // itself. A name this lane does not know constructs nothing and is reported: a
    // runner typo must fail loudly rather than build a half fixture that then
    // reads as a product defect.
    let fixture_kinds = match parse_fixture_kinds(std::env::var(FIXTURE_KINDS_ENV).ok().as_deref())
    {
        Ok(kinds) => kinds,
        Err(error) => {
            eprintln!("FERRYX_QA_FIXTURE_KINDS_REJECTED: {error}");
            Vec::new()
        }
    };
    let channel = Arc::clone(&boot.channel);
    {
        let fixture_channel = Arc::clone(&channel);
        let fixture_client = Arc::clone(&daemon_client);
        let fixture_app = app.clone();
        tauri::async_runtime::spawn(async move {
            // The settlement reads the surface host through this owned handle
            // (see `SurfaceHostObservations`), so the spawned future holds no
            // `State` borrow across an await and its `Send` proof never reaches
            // the WGPU object graph.
            let outcome = emit_gui_fixture_setup(
                &fixture_channel,
                &fixture_client,
                fixture_app,
                &fixture_kinds,
            )
            .await;
            if !outcome.emitted {
                eprintln!(
                    "FERRYX_QA_FIXTURE_SETUP_UNSETTLED: {} real session(s) observed but no operation nonce was supplied",
                    outcome.sessions.len()
                );
            }
        });
    }
    // The stale-binding watch calls an async method ON the managed surface host,
    // so its future holds `State<'_, NativeTerminalSurfaceHostState>` across its
    // awaits - a borrow no restructure in this file can drop, and one whose `Sync`
    // proof through the WGPU object graph overflows the default recursion limit
    // (`E0275` on `cargo build --features local-split-qa`). A dedicated thread
    // imposes no `Send` obligation on that future, and the closure captures only
    // owned `Send` data.
    let watch = std::thread::Builder::new()
        .name("ferryx-qa-stale-binding".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!("FERRYX_QA_STALE_BINDING_WATCH_UNSTARTED: runtime: {error}");
                    return;
                }
            };
            runtime.block_on(run_stale_binding_watcher(channel, daemon_client, app));
        });
    if let Err(error) = watch {
        eprintln!("FERRYX_QA_STALE_BINDING_WATCH_UNSTARTED: thread: {error}");
    }
}

/// Runner->product control that asks the product to offer a stale attempt against
/// the live binding (`barrierHub.command('trigger-stale-binding', ...)`).
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
const STALE_BINDING_COMMAND: &str = "trigger-stale-binding";
/// Bound on the private control watch. The runner triggers this right after
/// `fixture-setup`, so the window only stops a stray watcher from living forever.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
const STALE_BINDING_WATCH_MS: u64 = 60_000;
/// Matches `RELEASE_POLL_MS`: the private channel's own control-watch cadence.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
const STALE_BINDING_TICK_MS: u64 = 25;

/// Bounded watch for the runner's `trigger-stale-binding` control.
///
/// Driven by a dedicated thread (see `start_gui_boot_channel`) rather than spawned,
/// because the future below holds the managed surface host across its awaits.
///
/// The scenario pre-arms no barrier, so this command file is the only channel
/// between the runner and the product. The watch reads through the channel's own
/// nonce check, services each command at most once, and leaves a command it
/// cannot service yet pending for the next tick instead of answering it with an
/// invented outcome. Nothing here writes a receipt: the rejection and reattach
/// receipts come from the product's real fences, which the driver below drives.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
async fn run_stale_binding_watcher<R: tauri::Runtime>(
    channel: Arc<QaBarrierChannel>,
    daemon_client: Arc<crate::daemon::DaemonClient>,
    app: tauri::AppHandle<R>,
) {
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_millis(STALE_BINDING_WATCH_MS);
    let mut handled: Option<String> = None;
    let mut unserviced_reported = false;
    loop {
        if let Some(command) = channel.take_command(STALE_BINDING_COMMAND) {
            let issued_at = command
                .get("issuedAt")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if handled.as_deref() != Some(issued_at.as_str()) {
                let state = app.state::<NativeTerminalSurfaceHostState>();
                if state
                    .drive_stale_binding_command(&channel, Some(&daemon_client))
                    .await
                {
                    handled = Some(issued_at);
                } else if !unserviced_reported {
                    unserviced_reported = true;
                    eprintln!(
                        "FERRYX_QA_STALE_BINDING_UNSERVICED: no daemon session with a live seven-field binding yet; the command stays pending"
                    );
                }
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(STALE_BINDING_TICK_MS)).await;
    }
}

/// GUI-lane `fixture-setup`: construct the requested fixtures through the real
/// product paths, then settle line 0 from the real inventory.
///
/// The app handle is taken by value so the caller's spawned future holds an owned
/// `Send` handle instead of a `&AppHandle<R>`: the collector reads the surface host
/// through it, never through a managed-state borrow held across an await.
///
/// Creation and settlement share one deadline derived from the runner's own
/// `fixture-setup` budget, so a daemon that is slow to answer cannot push the
/// settlement past it: the truth of the attempt is always emitted, and a fixture
/// that could not be built is reported unbuilt with the daemon's own error.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
pub async fn emit_gui_fixture_setup<R: tauri::Runtime>(
    channel: &Arc<QaBarrierChannel>,
    daemon_client: &crate::daemon::DaemonClient,
    app: tauri::AppHandle<R>,
    kinds: &[QaFixtureKind],
) -> GuiFixtureSetupOutcome {
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_millis(FIXTURE_CREATE_BUDGET_MS);
    let creation = create_gui_fixture_sessions(
        daemon_client,
        kinds,
        channel.dir().join(FIXTURE_WORKSPACE_DIR),
        deadline,
    )
    .await;
    for (kind, hook) in &creation.unsupported {
        eprintln!(
            "FERRYX_QA_FIXTURE_KIND_UNSUPPORTED: kind={} hook={hook}",
            kind.as_str()
        );
    }
    for failure in &creation.failures {
        eprintln!("FERRYX_QA_FIXTURE_CREATE_FAILED: {failure}");
    }
    let inventory = collect_gui_fixture_sessions(daemon_client, app, &creation).await;
    for refusal in &inventory.refused {
        eprintln!("FERRYX_QA_FIXTURE_CLAIM_REFUSED: {refusal}");
    }
    let emitted =
        emit_fixture_setup_from_sessions_off_runtime(channel, &inventory, &creation).await;
    GuiFixtureSetupOutcome {
        emitted,
        sessions: inventory.sessions,
        creation,
    }
}

/// Constructs the fixture sessions the runner asked for, with the real product
/// paths and inside one deadline.
///
/// Every session here is a REAL daemon session created through the same request
/// the GUI issues: `source`/`idle` through `DaemonRequest::Spawn`
/// (`DaemonClient::spawn_terminal_with_startup`, the call `cmd_terminal_spawn`
/// makes for a local workspace), `created` through the local-split create path
/// (`prepare_local_split_until` -> `create_local_split_until`, the path
/// `cmd_terminal_spawn`'s create-only branch uses). Kinds this lane cannot
/// construct are reported with the exact hook they need; nothing is invented and
/// no session is created without the runner's private env.
#[cfg(all(feature = "local-split-qa", feature = "native-terminal"))]
async fn create_gui_fixture_sessions(
    client: &crate::daemon::DaemonClient,
    kinds: &[QaFixtureKind],
    fixture_root: PathBuf,
    deadline: tokio::time::Instant,
) -> GuiFixtureCreation {
    let started = tokio::time::Instant::now();
    let mut creation = GuiFixtureCreation {
        requested: kinds.to_vec(),
        ..Default::default()
    };
    for kind in kinds {
        if let FixtureCreation::Unsupported(hook) = kind.creation() {
            creation.unsupported.push((*kind, hook));
        }
    }
    let constructible: Vec<QaFixtureKind> = kinds
        .iter()
        .copied()
        .filter(|kind| !matches!(kind.creation(), FixtureCreation::Unsupported(_)))
        .collect();
    if !constructible.is_empty() {
        match register_fixture_workspace(client, fixture_root, deadline).await {
            Err(error) => {
                for kind in &constructible {
                    creation.failures.push(format!("{}: {error}", kind.as_str()));
                }
            }
            Ok(cwd) => {
                for kind in &constructible {
                    let created = match kind.creation() {
                        FixtureCreation::PlainSpawn => {
                            create_plain_fixture_session(client, &cwd, deadline).await
                        }
                        FixtureCreation::LocalSplit => {
                            create_split_fixture_session(client, &cwd, deadline).await
                        }
                        FixtureCreation::StoppedPlainSpawn => {
                            create_stopped_fixture_session(client, &cwd, deadline).await
                        }
                        // Filtered out above; reported rather than panicking if it
                        // ever is not.
                        FixtureCreation::Unsupported(hook) => Err(hook.to_string()),
                    };
                    match created {
                        Ok(session_id) => creation.assignments.push((session_id, *kind)),
                        Err(error) => creation.failures.push(format!("{}: {error}", kind.as_str())),
                    }
                }
            }
        }
    }
    creation.elapsed_ms = started.elapsed().as_millis() as u64;
    creation
}

/// Creates and registers the private fixture workspace.
///
/// The daemon refuses a spawn in a workspace it does not know, exactly as it does
/// for the GUI's own spawn, so the fixture workspace is registered through the
/// same `RegisterWorkspace` request the GUI's split preparation issues.
async fn register_fixture_workspace(
    client: &crate::daemon::DaemonClient,
    fixture_root: PathBuf,
    deadline: tokio::time::Instant,
) -> Result<String, String> {
    // Directory creation is blocking disk I/O: it runs off the async reactor like
    // every other filesystem step in this crate.
    let cwd = tokio::time::timeout_at(
        deadline,
        crate::ipc::run_blocking(move || {
            std::fs::create_dir_all(&fixture_root).map_err(crate::ipc::IpcError::internal)?;
            Ok(fixture_root.to_string_lossy().into_owned())
        }),
    )
    .await
    .map_err(|_| "fixture workspace directory timed out".to_string())?
    .map_err(|error| format!("fixture workspace directory failed: {error}"))?;
    tokio::time::timeout_at(
        deadline,
        client.register_workspace(FIXTURE_WORKSPACE_ID, &cwd),
    )
    .await
    .map_err(|_| "fixture workspace registration timed out".to_string())?
    .map_err(|error| format!("fixture workspace registration failed: {error:?}"))?;
    Ok(cwd)
}

/// One fixture session through the daemon's plain spawn: the same
/// `DaemonRequest::Spawn` the GUI issues when it opens a terminal.
async fn create_plain_fixture_session(
    client: &crate::daemon::DaemonClient,
    cwd: &str,
    deadline: tokio::time::Instant,
) -> Result<String, String> {
    let spawned = tokio::time::timeout_at(
        deadline,
        client.spawn_terminal_with_startup(
            uuid::Uuid::new_v4().to_string(),
            FIXTURE_WORKSPACE_ID.to_string(),
            None,
            Some(cwd.to_string()),
            FIXTURE_COLS,
            FIXTURE_ROWS,
            None,
            None,
        ),
    )
    .await
    .map_err(|_| "daemon spawn timed out".to_string())?
    .map_err(|error| format!("daemon spawn failed: {error:?}"))?;
    Ok(spawned.session_id)
}

/// One fixture session for the `externally-stopped` kind: the plain spawn, then a
/// real stop of its process through the PTY signal path.
///
/// The stop is verified before this returns. `fixture-setup` settles from the
/// daemon's own describe answer, so returning while the kernel has not yet applied
/// the stop would report a session that is about to be stopped as a running one -
/// the kind's whole claim is that it was observed stopped, so the observation is
/// what this waits for. The bounded wait keeps a stop that never lands a reported
/// failure instead of a fixture that silently is not what it says it is.
async fn create_stopped_fixture_session(
    client: &crate::daemon::DaemonClient,
    cwd: &str,
    deadline: tokio::time::Instant,
) -> Result<String, String> {
    let session_id = create_plain_fixture_session(client, cwd, deadline).await?;
    client
        .signal_terminal(&session_id, crate::terminal::TerminalSignal::Stop)
        .await
        .map_err(|error| format!("fixture external stop failed: {error:?}"))?;
    loop {
        match client.describe_session(&session_id).await {
            Ok(details) if details.kernel_stopped == Some(true) => return Ok(session_id),
            Ok(_) => {}
            Err(error) => {
                return Err(format!("fixture stop verification failed: {error:?}"))
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "fixture external stop was not observed by the daemon's own describe before the deadline"
                    .to_string(),
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// One fixture session through the local-split create path: the same two calls
/// `cmd_terminal_spawn`'s create-only branch makes, with this boot's own request
/// identity.
///
/// No `split-create` receipt is written for it: that receipt belongs to the
/// scenario's own trigger (the QA split producers in `ipc/terminal.rs`), and a
/// fixture settlement must never be mistaken for the scenario's split.
async fn create_split_fixture_session(
    client: &crate::daemon::DaemonClient,
    cwd: &str,
    deadline: tokio::time::Instant,
) -> Result<String, String> {
    let request_id = uuid::Uuid::new_v4().to_string();
    let identity = client
        .prepare_local_split_until(&request_id, deadline)
        .await
        .map_err(|error| format!("local split prepare failed: {error:?}"))?;
    let prepared = crate::daemon::protocol::PreparedLocalSplit {
        identity,
        workspace_id: FIXTURE_WORKSPACE_ID.to_string(),
        worktree: None,
        cwd: cwd.to_string(),
        shell: None,
        cols: FIXTURE_COLS,
        rows: FIXTURE_ROWS,
    };
    let created = client
        .create_local_split_until(&prepared, deadline)
        .await
        .map_err(|error| format!("local split create failed: {error:?}"))?;
    Ok(created.session_id)
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

// ---------------------------------------------------------------------------
// Off-runtime wrappers for this module's synchronous writers.
//
// Every writer below (`append_receipt`, `write_held`, `bind_target_session`,
// `emit_fixture_setup_from_sessions`, `scan_and_ack_arms`) performs synchronous
// file I/O and keeps its sync signature for its genuinely synchronous callers
// (the render path, the GUI boot entry, the headless lane that builds its own
// runtime). Async producers await the wrapper instead, so the write never runs
// on a worker of the runtime that serves them. Each wrapper preserves its sync
// sibling's own tolerances exactly: same file, same JSON, same discard shapes.
// ---------------------------------------------------------------------------

/// Off-runtime receipt append. Mirrors the private wrapper in
/// `terminal/qa_liveness.rs`.
pub(crate) async fn append_receipt_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    name: &str,
    operation_id: &str,
    settlement: Value,
) {
    let channel = Arc::clone(channel);
    let name = name.to_string();
    let operation_id = operation_id.to_string();
    // The sync append already tolerates every failure by returning without a
    // line; a failed blocking hop is the same non-event, never a fabricated
    // receipt.
    let _ = tokio::task::spawn_blocking(move || {
        channel.append_receipt(&name, &operation_id, settlement)
    })
    .await;
}

/// Off-runtime held-record emission (`<name>.held.json` plus the in-process
/// broadcast). The broadcast still fires from the blocking task, so the
/// ordering "file written, then event published" is the sync one.
pub(crate) async fn write_held_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    spec: &ArmSpec,
    session_id: &str,
    stage: &str,
    extra: Value,
) {
    let channel = Arc::clone(channel);
    let spec = spec.clone();
    let session_id = session_id.to_string();
    let stage = stage.to_string();
    // `write_held` returns `()` and already discards its own write error.
    let _ = tokio::task::spawn_blocking(move || {
        channel.write_held(&spec, &session_id, &stage, extra)
    })
    .await;
}

/// Off-runtime arm binding. A failed blocking hop is reported as a refusal
/// (the sync fn's own `Err` shape for an unarmed or conflicting barrier), never
/// as a bound ack: the caller treats `Err` as "not bound".
pub(crate) async fn bind_target_session_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    name: &str,
    operation_id: &str,
    session_id: &str,
) -> Result<QaBoundAck, String> {
    let channel = Arc::clone(channel);
    let name = name.to_string();
    let operation_id = operation_id.to_string();
    let session_id = session_id.to_string();
    match tokio::task::spawn_blocking(move || {
        channel.bind_target_session(&name, &operation_id, &session_id)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => Err(format!("bind_target_session blocking hop failed: {error}")),
    }
}

/// Off-runtime runner-bind adoption. The sync fn reads the runner's
/// `<name>.bind.json` with a synchronous read and then writes the bound-ack file
/// through `bind_target_session`; both run on the blocking pool here. A hop that
/// cannot complete is reported as a refusal (the same `Err` shape the sync fn
/// returns for an uncorrelated or conflicting bind), never as an adopted bind.
pub(crate) async fn adopt_runner_bind_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    name: &str,
) -> Result<String, String> {
    let channel = Arc::clone(channel);
    let name = name.to_string();
    match tokio::task::spawn_blocking(move || channel.adopt_runner_bind(&name)).await {
        Ok(result) => result,
        Err(error) => Err(format!("adopt_runner_bind blocking hop failed: {error}")),
    }
}

/// Off-runtime GUI `fixture-setup` settlement. The inventory and the creation
/// audit are cloned into the blocking task because the caller still reports
/// both after the settlement.
pub(crate) async fn emit_fixture_setup_from_sessions_off_runtime(
    channel: &Arc<QaBarrierChannel>,
    inventory: &GuiFixtureInventory,
    creation: &GuiFixtureCreation,
) -> bool {
    let channel = Arc::clone(channel);
    let inventory = inventory.clone();
    let creation = creation.clone();
    match tokio::task::spawn_blocking(move || {
        channel.emit_fixture_setup_from_sessions(&inventory, &creation)
    })
    .await
    {
        Ok(emitted) => emitted,
        // A hop that cannot complete emitted no settlement: the same `false`
        // the sync fn returns when no operation nonce correlates the line.
        Err(_) => false,
    }
}

/// Off-runtime startup arm scan (both a directory read and the armed-ack
/// writes). A hop that cannot complete yields the same tuple the sync fn
/// produces when it cannot read the control directory at all (`:663-665`):
/// nothing acked, nothing rejected. The daemon then holds no barrier instead of
/// assuming one was armed, which fails toward not-holding, never toward a pass.
pub(crate) async fn scan_and_ack_arms_off_runtime(
    channel: &Arc<QaBarrierChannel>,
) -> (Vec<String>, Vec<String>) {
    let channel = Arc::clone(channel);
    match tokio::task::spawn_blocking(move || channel.scan_and_ack_arms()).await {
        Ok(result) => result,
        Err(_) => (Vec::new(), Vec::new()),
    }
}

/// The operation identity this write belongs to, or `None` when the barrier does
/// not own it.
///
/// Two identities are honoured, and which one applies is the runner's choice:
///
/// * a barrier the runner BOUND to a concrete session (`bindBackendSession` ->
///   `<name>.bind.json`) owns that session's write. This is the only identity a
///   real keystroke can satisfy: the operation nonce lives in the product's
///   inherited env and in the arm file, while a keystroke's request id is minted
///   by the frontend input queue (`req-<queueRunId>-<sessionId>-<itemId>`,
///   `ui/src/lib/nativeTerminalInputQueue.ts`), so the two can never be equal on
///   the GUI lane. The `presentation` barrier has always been addressed by the
///   session the runner bound; the write stage now is too.
/// * an UNBOUND barrier keeps the operation-nonce path unchanged, which is how
///   the headless lane drives this stage.
fn write_barrier_operation<'a>(
    spec: &'a ArmSpec,
    session_id: &str,
    operation_id: Option<&'a str>,
) -> Option<&'a str> {
    match spec.target_backend_session_id.as_deref() {
        Some(target) if !target.is_empty() => {
            (target == session_id).then_some(spec.operation_id.as_str())
        }
        _ => match operation_id {
            Some(id) if id == spec.operation_id => Some(id),
            _ => None,
        },
    }
}

/// Hold the REAL backend-write stage: called from
/// `send_native_terminal_input_with_stage_logging` after the
/// `backend_write_start` stage event and before the write future is awaited.
/// Returns `Some(outcome)` only when this exact operation drove the barrier.
pub(crate) async fn hold_backend_write_barrier(
    channel: &Arc<QaBarrierChannel>,
    state: &NativeTerminalSurfaceHostState,
    session_id: &str,
    operation_id: Option<&str>,
) -> Option<ReleaseOutcome> {
    let mut spec = channel.spec(WRITE_BARRIER)?;
    // The runner addresses a barrier by binding it to the session it created
    // (`bindBackendSession` -> `<name>.bind.json`). The presentation stage adopts
    // that bind on its render path; this stage has no render path of its own, so
    // the bind is adopted here, once, the first time a write reaches an
    // armed-but-unbound barrier. An adoption that cannot be correlated reports
    // itself and leaves the barrier on its operation-nonce path unchanged.
    if spec.target_backend_session_id.as_deref().map_or(true, str::is_empty) {
        if adopt_runner_bind_off_runtime(channel, WRITE_BARRIER).await.is_ok() {
            if let Some(adopted) = channel.spec(WRITE_BARRIER) {
                spec = adopted;
            }
        }
    }
    let operation_id = write_barrier_operation(&spec, session_id, operation_id)?;
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
    write_held_off_runtime(
        channel,
        &spec,
        session_id,
        "backend_write_start",
        json!({
            "writePendingMs": pending_ms,
            "classifierVerdict": verdict_str(held_verdict),
        }),
    )
    .await;
    append_receipt_off_runtime(
        channel,
        WRITE_BARRIER,
        operation_id,
        json!({
            "sessionId": session_id,
            "stage": "backend_write_start",
            "classifierVerdict": verdict_str(held_verdict),
            "writePendingMs": pending_ms,
            "snapshot": QaBarrierChannel::snapshot_json(&held_snapshot),
        }),
    )
    .await;
    Some(channel.wait_for_release(&spec).await)
}

/// Settle the backend-write barrier after the real write future completed.
/// The receipt carries the fresh collector snapshot plus actual stage-progress
/// evidence; the verdict is whatever the real classifier returns from that
/// snapshot. `Idle` is never forced - an `Unknown` verdict is reported with
/// `evidenceMissing: true` and the exact missing-field list.
///
/// The classifier's positive verdict also needs the daemon's own reader/kernel
/// facts, which this layer does not own. When a QA daemon client is installed
/// (the GUI lane) they are observed for real before the receipt is written;
/// without one (the headless lane) the settlement is written exactly as before
/// and `daemonFacts` is `null`. The settlement's receipt append is synchronous
/// file I/O, so this entry point is `async` and awaits the off-runtime wrapper.
pub(crate) async fn settle_backend_write_barrier(
    channel: &Arc<QaBarrierChannel>,
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
    let Some(operation_id) = write_barrier_operation(&spec, session_id, operation_id) else {
        return;
    };
    let mut fresh_snapshot =
        write_stage_snapshot(state, session_id, operation_id, None, None, Some(success));
    if let Some(client) = qa_daemon_client() {
        let session_id = session_id.to_string();
        let operation_id = operation_id.to_string();
        let channel = Arc::clone(channel);
        tauri::async_runtime::spawn(async move {
            let facts = observe_daemon_liveness(&client, &session_id).await;
            facts.apply(&mut fresh_snapshot);
            append_backend_write_settlement(
                &channel,
                &operation_id,
                &session_id,
                fresh_snapshot,
                success,
                duration_ms,
                outcome,
                Some(facts),
            )
            .await;
        });
        return;
    }
    append_backend_write_settlement(
        channel,
        operation_id,
        session_id,
        fresh_snapshot,
        success,
        duration_ms,
        outcome,
        None,
    )
    .await;
}

/// One `backend-write` settlement line: the fresh collector snapshot, the real
/// classifier verdict over it, and the daemon facts that were really observed
/// (`null` when this process holds no daemon client, e.g. the headless lane).
/// The receipt append itself is synchronous file I/O, so it is awaited through
/// the off-runtime wrapper rather than executed on a runtime worker.
async fn append_backend_write_settlement(
    channel: &Arc<QaBarrierChannel>,
    operation_id: &str,
    session_id: &str,
    fresh_snapshot: PaneLivenessSnapshot,
    success: bool,
    duration_ms: f64,
    outcome: Option<ReleaseOutcome>,
    daemon_facts: Option<QaDaemonLiveness>,
) {
    let verdict = classify_pane_liveness(&fresh_snapshot);
    let missing = QaBarrierChannel::evidence_missing_fields(&fresh_snapshot);
    append_receipt_off_runtime(
        channel,
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
            "daemonFacts": daemon_facts
                .map(|facts| serde_json::to_value(facts).unwrap_or(Value::Null)),
            "snapshot": QaBarrierChannel::snapshot_json(&fresh_snapshot),
        }),
    )
    .await;
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
        assert!(channel.emit_fixture_setup_from_sessions(
            &GuiFixtureInventory::from_sessions(Vec::new()),
            &GuiFixtureCreation::default()
        ));
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
                stop_probe_state: None,
                ownership_receipt: json!({"backendSessionId": "pty-real-1", "daemonEpoch": "42"}),
            },
            QaFixtureSession {
                backend_session_id: "pty-real-2".to_string(),
                kind: "source".to_string(),
                stop_probe_state: None,
                ownership_receipt: json!({"backendSessionId": "pty-real-2", "incarnation": "inc-2"}),
            },
            QaFixtureSession {
                backend_session_id: "pty-real-3".to_string(),
                kind: "externally-stopped".to_string(),
                stop_probe_state: Some("stopped".to_string()),
                ownership_receipt: json!({
                    "backendSessionId": "pty-real-3",
                    "kernelStopped": true,
                    "registrySuspended": false,
                }),
            },
        ];
        assert!(channel.emit_fixture_setup_from_sessions(
            &GuiFixtureInventory::from_sessions(observed.clone()),
            &GuiFixtureCreation::default()
        ));
        let lines = read_lines(&dir, "fixture-setup");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1]["sessionId"], json!("pty-real-1"));
        assert_eq!(lines[1]["fixtureKind"], json!("gui-session-inventory"));
        assert_eq!(lines[1]["sessions"][0]["backendSessionId"], json!("pty-real-1"));
        assert_eq!(lines[1]["sessions"][0]["kind"], json!("idle"));
        assert_eq!(lines[1]["sessions"][1]["ownershipReceipt"]["incarnation"], json!("inc-2"));
        assert_eq!(lines[1]["sessions"][2]["kind"], json!("externally-stopped"));
        assert_eq!(lines[1]["sessions"][2]["stopProbeState"], json!("stopped"));
        // Only the externally-stopped session carries a stop probe, so the
        // runner's externally-stopped validator can never be satisfied by an
        // idle/source session.
        assert!(lines[1]["sessions"][0].get("stopProbeState").is_none());
        assert!(lines[1]["sessions"][1].get("stopProbeState").is_none());
        // The kinds the product cannot attest are never claimed.
        for line in &lines {
            for session in line["sessions"].as_array().unwrap() {
                let kind = session["kind"].as_str().unwrap();
                assert!(
                    matches!(kind, "idle" | "source" | "externally-stopped"),
                    "GUI inventory must not fabricate fixture kinds: {kind}"
                );
            }
        }

        // Without a correlation identity the line is refused instead of
        // written uncorrelatable.
        let uncorrelated = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        assert!(!uncorrelated.emit_fixture_setup_from_sessions(
            &GuiFixtureInventory::from_sessions(observed.clone()),
            &GuiFixtureCreation::default()
        ));
        assert_eq!(read_lines(&dir, "fixture-setup").len(), 2);
    }

    // The command consumer: only a control that echoes THIS run's nonces is
    // returned, so a stale or replayed command from another run cannot drive the
    // product - the same rule the release controls already enforce.
    #[test]
    fn take_command_honors_the_run_and_operation_nonces() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = QaBarrierChannel::from_env_values(
            dir.to_str().unwrap(),
            TEST_RUN_ID,
            Some(TEST_OPERATION_ID),
        )
        .unwrap();
        assert_eq!(channel.dir(), dir.as_path());

        let name = "trigger-stale-binding";
        let command = |run_id: &str, operation_id: &str| {
            json!({
                "name": name,
                "runId": run_id,
                "operationId": operation_id,
                "issuedAt": "2026-10-04T00:00:00.000Z",
                "mutateField": "attemptGeneration",
            })
        };

        assert!(channel.take_command(name).is_none(), "no command file yet");
        std::fs::write(
            dir.join(format!("{name}.request.json")),
            serde_json::to_string(&command("qa-run-OTHER", TEST_OPERATION_ID)).unwrap(),
        )
        .unwrap();
        assert!(
            channel.take_command(name).is_none(),
            "another run's command is refused"
        );
        std::fs::write(
            dir.join(format!("{name}.request.json")),
            serde_json::to_string(&command(TEST_RUN_ID, "qa-op-OTHER")).unwrap(),
        )
        .unwrap();
        assert!(
            channel.take_command(name).is_none(),
            "another operation's command is refused"
        );
        std::fs::write(
            dir.join(format!("{name}.request.json")),
            serde_json::to_string(&command(TEST_RUN_ID, TEST_OPERATION_ID)).unwrap(),
        )
        .unwrap();
        let taken = channel.take_command(name).expect("correlated command");
        assert_eq!(taken["mutateField"], json!("attemptGeneration"));
        assert_eq!(taken["issuedAt"], json!("2026-10-04T00:00:00.000Z"));
        // Reading is not consuming: the caller decides when a command is served.
        assert!(channel.take_command(name).is_some());
        assert!(dir.join(format!("{name}.request.json")).exists());
    }

    // The runner's live-arm binding: adopted only when it is correlated, concrete
    // and does not conflict with a target the product already bound.
    #[test]
    fn runner_bind_is_adopted_only_when_correlated_and_unconflicted() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, PRESENTATION_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = QaBarrierChannel::from_env_values(
            dir.to_str().unwrap(),
            TEST_RUN_ID,
            Some(TEST_OPERATION_ID),
        )
        .unwrap();
        channel.scan_and_ack_arms();

        let bind = |run_id: &str, operation_id: &str, target: &str| {
            json!({
                "name": PRESENTATION_BARRIER,
                "runId": run_id,
                "operationId": operation_id,
                "targetBackendSessionId": target,
                "clientRequestId": null,
                "boundAt": "2026-10-04T00:00:00.000Z",
            })
        };
        let write_bind = |value: &Value| {
            std::fs::write(
                dir.join(format!("{PRESENTATION_BARRIER}.bind.json")),
                serde_json::to_string(value).unwrap(),
            )
            .unwrap();
        };

        // No bind file: the barrier stays unbound and the caller reports it.
        assert!(channel.adopt_runner_bind(PRESENTATION_BARRIER).is_err());
        assert!(channel
            .target_backend_session_id_for(PRESENTATION_BARRIER)
            .is_none());

        // A bind from another run is refused.
        write_bind(&bind("qa-run-OTHER", TEST_OPERATION_ID, "backend-real-1"));
        assert!(channel.adopt_runner_bind(PRESENTATION_BARRIER).is_err());
        // A wildcard target is refused by the channel's own binding rule.
        write_bind(&bind(TEST_RUN_ID, TEST_OPERATION_ID, "*"));
        assert!(channel.adopt_runner_bind(PRESENTATION_BARRIER).is_err());
        assert!(channel
            .target_backend_session_id_for(PRESENTATION_BARRIER)
            .is_none());

        // The correlated concrete bind is installed and acknowledged.
        write_bind(&bind(TEST_RUN_ID, TEST_OPERATION_ID, "backend-real-1"));
        assert_eq!(
            channel.adopt_runner_bind(PRESENTATION_BARRIER).unwrap(),
            "backend-real-1"
        );
        assert_eq!(
            channel
                .target_backend_session_id_for(PRESENTATION_BARRIER)
                .as_deref(),
            Some("backend-real-1")
        );
        let ack: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{PRESENTATION_BARRIER}.bound-ack.json")))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(ack["targetBackendSessionId"], json!("backend-real-1"));
        assert_eq!(ack["runId"], json!(TEST_RUN_ID));

        // A later conflicting bind is refused: an installed target is never
        // redirected by a control file.
        write_bind(&bind(TEST_RUN_ID, TEST_OPERATION_ID, "backend-other"));
        assert!(channel.adopt_runner_bind(PRESENTATION_BARRIER).is_err());
        assert_eq!(
            channel
                .target_backend_session_id_for(PRESENTATION_BARRIER)
                .as_deref(),
            Some("backend-real-1")
        );
    }

    // The daemon facts: an unobserved field must stay unobserved (the classifier
    // reads `false` as "verified not blocked"), and only a real observation may
    // turn the verdict positive.
    #[test]
    fn daemon_facts_are_only_reported_when_observed() {
        let older_daemon = crate::daemon::protocol::DaemonSessionDetails::new(
            "backend-1".into(),
            None,
            None,
            None,
            80,
            24,
            true,
            None,
            None,
            None,
            false,
        );
        let unobserved = daemon_liveness_from_details(&older_daemon, None);
        assert!(unobserved.observed);
        assert_eq!(unobserved.reader_paused, None);
        assert_eq!(unobserved.kernel_stopped, None);
        assert_eq!(unobserved.suspended, Some(false));

        let mut snapshot = PaneLivenessSnapshot {
            telemetry_available: true,
            session_id: Some("backend-1".into()),
            vt_session_id: Some("backend-1".into()),
            has_unpresented_frames: Some(false),
            ..Default::default()
        };
        unobserved.apply(&mut snapshot);
        assert_eq!(snapshot.reader_paused, None);
        assert_eq!(snapshot.kernel_stopped, None);
        // A daemon that does not report the reader/kernel facts cannot produce a
        // positive verdict, and the missing fields are named.
        assert_eq!(
            classify_pane_liveness(&snapshot),
            PaneLivenessVerdict::Unknown
        );
        let missing = QaBarrierChannel::evidence_missing_fields(&snapshot);
        assert!(missing.contains(&"readerPaused"));
        assert!(missing.contains(&"kernelStopped"));

        // The same snapshot carrying the daemon's real answer is idle.
        let mut answered_daemon = older_daemon.clone();
        answered_daemon.reader_paused = Some(false);
        answered_daemon.kernel_stopped = Some(false);
        daemon_liveness_from_details(&answered_daemon, Some(7)).apply(&mut snapshot);
        assert_eq!(snapshot.daemon_epoch.as_deref(), Some("7"));
        assert_eq!(classify_pane_liveness(&snapshot), PaneLivenessVerdict::Idle);
    }

    // The runner's fixture request: an unknown kind is refused instead of being
    // silently dropped, duplicates collapse to the one session per kind the
    // runner's validator asks for, and the kinds this lane cannot construct stay
    // parseable because they are reported (with their hook), never invented.
    #[test]
    fn fixture_kind_requests_are_parsed_and_unknown_names_refused() {
        assert!(parse_fixture_kinds(None).unwrap().is_empty());
        assert!(parse_fixture_kinds(Some("   ")).unwrap().is_empty());
        assert_eq!(
            parse_fixture_kinds(Some(" source , created ,idle ")).unwrap(),
            vec![
                QaFixtureKind::Source,
                QaFixtureKind::Created,
                QaFixtureKind::Idle
            ]
        );
        assert_eq!(
            parse_fixture_kinds(Some("idle,idle")).unwrap(),
            vec![QaFixtureKind::Idle]
        );
        assert_eq!(
            parse_fixture_kinds(Some("adopted,externally-stopped")).unwrap(),
            vec![QaFixtureKind::Adopted, QaFixtureKind::ExternallyStopped]
        );
        let error = parse_fixture_kinds(Some("source,bogus")).unwrap_err();
        assert!(
            error.contains("bogus"),
            "the unknown kind must be named: {error}"
        );
    }

    // Only the kinds with a real product path are constructible here; the one that
    // is not must report its own hook, never a session this lane cannot attest.
    #[test]
    fn unconstructible_fixture_kinds_report_a_hook_instead_of_a_session() {
        assert_eq!(QaFixtureKind::Source.creation(), FixtureCreation::PlainSpawn);
        assert_eq!(QaFixtureKind::Idle.creation(), FixtureCreation::PlainSpawn);
        // `created` is the only kind with local-split-create provenance.
        assert_eq!(QaFixtureKind::Created.creation(), FixtureCreation::LocalSplit);
        // `externally-stopped` is constructible: a plain spawn whose process is then
        // really stopped through the PTY signal path, which never touches the
        // suspension ownership ledger and so is honestly unowned.
        assert_eq!(
            QaFixtureKind::ExternallyStopped.creation(),
            FixtureCreation::StoppedPlainSpawn
        );

        let mut hooks = Vec::new();
        for kind in [QaFixtureKind::Adopted] {
            match kind.creation() {
                FixtureCreation::Unsupported(hook) => hooks.push(hook),
                other => panic!("{} must not be constructible: {other:?}", kind.as_str()),
            }
        }
        assert!(hooks.iter().all(|hook| !hook.is_empty()));
        assert_ne!(hooks[0], hooks[1], "each unsupported kind names its own hook");
    }

    // The kind claims: an observation the daemon really reported outranks an
    // intended kind, an intention the daemon's reply does not support is refused
    // (never downgraded silently), and the kinds this lane cannot construct are
    // never invented.
    #[test]
    fn fixture_kind_claims_follow_the_daemons_own_reply() {
        let base = || {
            crate::daemon::protocol::DaemonSessionDetails::new(
                "backend-1".into(),
                None,
                None,
                None,
                80,
                24,
                true,
                None,
                None,
                None,
                false,
            )
        };
        let idle_facts = || crate::daemon::protocol::DaemonSessionDetails {
            reader_paused: Some(false),
            kernel_stopped: Some(false),
            ..base()
        };

        // 1. A real external stop is an observation: it is reported whatever kind
        //    was asked for, with the stop probe the runner's validator requires.
        let stopped_and_unowned = crate::daemon::protocol::DaemonSessionDetails {
            kernel_stopped: Some(true),
            registry_suspended: Some(false),
            ..base()
        };
        let external = classify_fixture_session(
            &stopped_and_unowned,
            None,
            Some(QaFixtureKind::Created),
        );
        assert_eq!(external.kind, "externally-stopped");
        assert_eq!(external.stop_probe_state, Some("stopped"));
        assert_eq!(external.basis, FixtureKindBasis::KernelStopUnowned);

        // 2. A stop the daemon's registry owns, or one it cannot attribute, is
        //    never claimed as external.
        let stopped_and_owned = crate::daemon::protocol::DaemonSessionDetails {
            kernel_stopped: Some(true),
            registry_suspended: Some(true),
            ..base()
        };
        assert_eq!(
            classify_fixture_session(&stopped_and_owned, None, None).kind,
            "source"
        );
        let stopped_unattributed = crate::daemon::protocol::DaemonSessionDetails {
            kernel_stopped: Some(true),
            ..base()
        };
        let unattributed = classify_fixture_session(&stopped_unattributed, None, None);
        assert_eq!(unattributed.kind, "source");
        assert_eq!(unattributed.stop_probe_state, None);
        let explicitly_external = crate::daemon::protocol::DaemonSessionDetails {
            kernel_stopped: Some(true),
            suspension_source: Some("external-kernel".into()),
            ..base()
        };
        assert_eq!(
            classify_fixture_session(&explicitly_external, None, None).kind,
            "externally-stopped"
        );

        // 3. `idle` only from the daemon's real facts - plus the surface host's own
        //    observation when this process has one, which vetoes the claim - and
        //    never from a defaulted field.
        let idle = classify_fixture_session(&idle_facts(), None, Some(QaFixtureKind::Idle));
        assert_eq!(idle.kind, "idle");
        assert_eq!(idle.basis, FixtureKindBasis::DaemonIdleFacts);
        let unobserved = classify_fixture_session(&base(), None, Some(QaFixtureKind::Idle));
        assert_eq!(unobserved.kind, "source");
        assert_eq!(
            unobserved.refusal,
            Some("daemon-did-not-report-reader-and-kernel-facts")
        );
        let frames_pending = PaneLivenessSnapshot {
            has_unpresented_frames: Some(true),
            ..Default::default()
        };
        let vetoed = classify_fixture_session(
            &idle_facts(),
            Some(&frames_pending),
            Some(QaFixtureKind::Idle),
        );
        assert_eq!(vetoed.kind, "source");
        assert_eq!(
            vetoed.refusal,
            Some("surface-host-observes-unpresented-frames")
        );
        let frames_clear = PaneLivenessSnapshot {
            has_unpresented_frames: Some(false),
            ..Default::default()
        };
        let with_host = classify_fixture_session(
            &idle_facts(),
            Some(&frames_clear),
            Some(QaFixtureKind::Idle),
        );
        assert_eq!(with_host.kind, "idle");
        assert_eq!(with_host.basis, FixtureKindBasis::DaemonIdleFactsAndSurfaceHost);

        // 4. `created` is claimed only from this boot's own split-create
        //    provenance. With no intention at all the session is reported as the
        //    daemon's own reply describes it - an observation is never erased by
        //    the absence of an intention, so full idle facts stay `idle` instead of
        //    collapsing to the `source` fallback.
        let created = classify_fixture_session(&idle_facts(), None, Some(QaFixtureKind::Created));
        assert_eq!(created.kind, "created");
        assert_eq!(created.basis, FixtureKindBasis::QaLocalSplitCreate);
        let unclaimed = classify_fixture_session(&idle_facts(), None, None);
        assert_eq!(unclaimed.kind, "idle");
        assert_eq!(unclaimed.basis, FixtureKindBasis::DaemonIdleFacts);
        assert_eq!(unclaimed.stop_probe_state, None);
        assert_eq!(unclaimed.refusal, None);

        // 5. The kind this lane cannot construct is refused with a reason.
        let refused = classify_fixture_session(&base(), None, Some(QaFixtureKind::Adopted));
        assert_eq!(refused.refusal, Some("kind-not-constructible-in-this-lane"));
        assert_eq!(refused.kind, "source");
    }

    // The settlement carries the construction audit, so a kind that could not be
    // built is visible in the receipt instead of being silently absent.
    #[test]
    fn fixture_settlement_carries_the_creation_audit() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = QaBarrierChannel::from_env_values(
            dir.to_str().unwrap(),
            TEST_RUN_ID,
            Some(TEST_OPERATION_ID),
        )
        .unwrap();

        let inventory = GuiFixtureInventory::from_sessions(vec![QaFixtureSession {
            backend_session_id: "pty-real-1".to_string(),
            kind: "source".to_string(),
            stop_probe_state: None,
            ownership_receipt: json!({
                "backendSessionId": "pty-real-1",
                "fixtureKindBasis": "daemon-reports-running",
                "qaCreated": true,
            }),
        }]);
        let creation = GuiFixtureCreation {
            requested: vec![QaFixtureKind::Source, QaFixtureKind::Adopted],
            assignments: vec![("pty-real-1".to_string(), QaFixtureKind::Source)],
            unsupported: vec![(QaFixtureKind::Adopted, "needs a real retained handover")],
            failures: vec!["created: local split create failed: timeout".to_string()],
            elapsed_ms: 412,
        };

        assert!(channel.emit_fixture_setup_from_sessions(&inventory, &creation));
        let lines = read_lines(&dir, "fixture-setup");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["sessionId"], json!("pty-real-1"));
        assert_eq!(lines[0]["fixtureKindsRequested"], json!(["source", "adopted"]));
        assert_eq!(lines[0]["fixtureKindsClaimed"], json!(["source"]));
        assert_eq!(lines[0]["fixtureKindsUnsupported"][0]["kind"], json!("adopted"));
        assert_eq!(
            lines[0]["fixtureKindsUnsupported"][0]["hook"],
            json!("needs a real retained handover")
        );
        assert_eq!(
            lines[0]["fixtureCreationFailures"],
            json!(["created: local split create failed: timeout"])
        );
        assert_eq!(lines[0]["fixtureCreationElapsedMs"], json!(412));
        assert_eq!(
            lines[0]["sessions"][0]["ownershipReceipt"]["fixtureKindBasis"],
            json!("daemon-reports-running")
        );

        // Without a correlation identity the settlement is refused instead of
        // written uncorrelatable.
        let uncorrelated = QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string());
        assert!(!uncorrelated.emit_fixture_setup_from_sessions(&inventory, &creation));
        assert_eq!(read_lines(&dir, "fixture-setup").len(), 1);
    }

    // The creation path against a REAL daemon on a private socket: `source`/`idle`
    // come from the plain spawn, `created` from the local-split create path, and
    // the settlement claims each kind from the daemon's own reply about the
    // session this boot really created.
    #[cfg(unix)]
    #[tokio::test]
    async fn fixture_creation_uses_the_real_daemon_paths() {
        use crate::daemon::DaemonServer;

        let dir = tempfile::tempdir().unwrap();
        let socket_path = dir.path().join("qa-fixture.sock");
        let listener = tokio::net::UnixListener::bind(&socket_path).unwrap();
        let server = Arc::new(DaemonServer::new());
        let server_task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let server = Arc::clone(&server);
                        tokio::spawn(async move {
                            server.handle_client(stream).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        });
        let client = crate::daemon::DaemonClient::new_with_socket(socket_path);

        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        let kinds = vec![
            QaFixtureKind::Source,
            QaFixtureKind::Idle,
            QaFixtureKind::Created,
        ];
        let creation = create_gui_fixture_sessions(
            &client,
            &kinds,
            dir.path().join(FIXTURE_WORKSPACE_DIR),
            deadline,
        )
        .await;
        assert!(
            creation.failures.is_empty(),
            "creation failed: {:?}",
            creation.failures
        );
        assert!(creation.unsupported.is_empty());
        assert_eq!(creation.assignments.len(), 3);

        let session_for = |kind: QaFixtureKind| {
            creation
                .assignments
                .iter()
                .find(|(_, assigned)| *assigned == kind)
                .map(|(session_id, _)| session_id.clone())
                .unwrap_or_else(|| panic!("no session was created for {}", kind.as_str()))
        };
        let source_id = session_for(QaFixtureKind::Source);
        let idle_id = session_for(QaFixtureKind::Idle);
        let created_id = session_for(QaFixtureKind::Created);
        assert_ne!(source_id, idle_id);
        assert_ne!(source_id, created_id);
        assert_ne!(idle_id, created_id);

        let inventory = collect_gui_fixture_sessions(
            &client,
            &NativeTerminalSurfaceHostState::default(),
            &creation,
        )
        .await;
        assert!(inventory.refused.is_empty(), "refused: {:?}", inventory.refused);
        assert!(inventory.not_running.is_empty());
        assert_eq!(inventory.sessions.len(), 3);

        let session = |session_id: &str| {
            inventory
                .sessions
                .iter()
                .find(|session| session.backend_session_id == session_id)
                .unwrap_or_else(|| panic!("session {session_id} is missing from the inventory"))
        };
        assert_eq!(session(&source_id).kind, "source");
        assert_eq!(session(&idle_id).kind, "idle");
        assert_eq!(session(&created_id).kind, "created");

        // The evidence each claim rests on, and the real receipt fields: every
        // fixture carries its backend session id, the daemon's epoch and the
        // basis its kind was claimed from.
        let basis = |session_id: &str| {
            session(session_id).ownership_receipt["fixtureKindBasis"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        };
        assert_eq!(basis(&source_id), "daemon-reports-running");
        assert_eq!(basis(&idle_id), "daemon-idle-facts");
        assert_eq!(basis(&created_id), "qa-local-split-create");
        for fixture in &inventory.sessions {
            assert_eq!(
                fixture.ownership_receipt["backendSessionId"],
                json!(fixture.backend_session_id)
            );
            assert!(fixture.ownership_receipt["daemonEpoch"].is_string());
            assert_eq!(fixture.ownership_receipt["qaCreated"], json!(true));
            assert!(fixture.stop_probe_state.is_none());
        }

        server_task.abort();
    }

    // Without a daemon client (the headless lane) the write settlement reports no
    // daemon facts at all instead of defaulted ones, and keeps the honest
    // non-pass.
    #[tokio::test]
    async fn backend_write_settlement_reports_no_unobserved_daemon_facts() {
        assert!(
            qa_daemon_client().is_none(),
            "this test asserts the no-client path; no test may install a client first"
        );
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        arm(&dir, WRITE_BARRIER, TEST_RUN_ID, TEST_OPERATION_ID);
        let channel = Arc::new(QaBarrierChannel::new(dir.clone(), TEST_RUN_ID.to_string()));
        channel.scan_and_ack_arms();
        let state = NativeTerminalSurfaceHostState::default();

        settle_backend_write_barrier(
            &channel,
            &state,
            "qa-headless-write",
            Some(TEST_OPERATION_ID),
            Some(ReleaseOutcome::Released),
            true,
            12.5,
        )
        .await;

        let lines = read_lines(&dir, WRITE_BARRIER);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["stage"], json!("backend_write_settled"));
        assert_eq!(lines[0]["daemonFacts"], Value::Null);
        assert_eq!(lines[0]["stageProgress"]["backendWriteCompleted"], json!(true));
        assert_eq!(lines[0]["classifierVerdict"], json!("Unknown"));
        assert_eq!(lines[0]["evidenceMissing"], json!(true));
        let missing = lines[0]["evidenceMissingFields"].as_array().unwrap();
        for field in ["readerPaused", "kernelStopped", "suspended"] {
            assert!(
                missing.iter().any(|entry| entry == field),
                "{field} must be reported as missing"
            );
        }
    }
}
