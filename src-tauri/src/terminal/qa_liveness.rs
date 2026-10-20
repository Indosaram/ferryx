//! Private QA liveness producers owned by the terminal (daemon) layer.
//!
//! Runner contract (`scripts/qa/pane-liveness.mjs` `SCENARIO_PLANS`):
//!
//! - `marker-output` — the terminal layer owns this receipt, its fields and its
//!   line indices. `output`/`outputSequence` come from the session's real output
//!   hub, `ptyCreatedCount` from the real local-PTY accounting of this process,
//!   and `frameSubmitted` from the native lane's real frame-submission sidecar
//!   ([`FRAME_SUBMITTED_FILE`], written by `native_terminal/surface_host.rs`
//!   with the hub sequence each submitted frame really covers). A marker receipt
//!   is settled only once that evidence reaches the occurrence's own sequence;
//!   the negative is reported truthfully, never upgraded.
//! - `suspension-receipt` — settled only after a successful identity-verified OS
//!   actuation, carrying the ownership classification of a Ferryx-owned stop
//!   (actuated, then auto-resumed on the same process) and of an external stop
//!   (observed, never claimed, never auto-resumed).
//! - `eof-handled` — the runner's `exercise-eof` control ends ONE output stream
//!   this lane owns. The stream is a real local PTY spawned through the
//!   production service path (never the runner's own pane), subscribed through
//!   the production attach path before it ends, and ended by letting its shell
//!   exit, so the PTY reader reaches its real end of file. The receipt is settled
//!   from what the product really did next: the output pump finishing (the
//!   daemon's own end-of-stream signal), the hub's EOF path closing that
//!   subscription, the lifecycle watcher finalizing the natural exit, and this
//!   daemon's own PTY accounting proving no replacement PTY was created. A
//!   stream whose end cannot be observed settles nothing.
//!
//! Gating: the whole module is compiled only with `local-split-qa` +
//! `native-terminal`, and every producer additionally requires the private
//! channel the runner installed from `FERRYX_QA_BARRIER_DIR` / `FERRYX_QA_RUN_ID`
//! (plus `FERRYX_QA_OPERATION_ID` for the scenarios that pre-arm no barrier).
//! A normal build has no QA surface at all.
//!
//! Process topology: PTYs, their output hub and the suspension ledger live in
//! the daemon process, while native frames are submitted by the GUI process.
//! [`start`] installs the channel for the daemon process (the GUI installs its
//! own from the same inherited env), so both halves of the marker receipt meet
//! in the shared channel directory.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};

use super::service::auto_resume_suspension;
use super::suspension::{classify_stop_source, resume_owned, SuspensionSource, SuspensionTarget};
use super::{PtySession, TerminalService};
use crate::ipc::qa_barrier::{self, QaBarrierChannel};

pub(crate) const MARKER_OUTPUT_RECEIPT: &str = "marker-output";
/// Receipt the runner awaits for the suspension ownership classification.
pub(crate) const SUSPENSION_RECEIPT: &str = "suspension-receipt";
/// Runner->product control that triggers the suspension ownership check.
const SUSPENSION_CHECK_COMMAND: &str = "trigger-suspension-check";
/// Receipt the runner awaits for the real EOF handling of an owned stream.
pub(crate) const EOF_HANDLED_RECEIPT: &str = "eof-handled";
/// Runner->product control that ends one QA-owned output stream. The runner
/// issues it after the classifier stages and asserts two things about the
/// settlement: the product really observed the EOF (`eofObserved`) and the EOF
/// handling created no replacement PTY (`newPtyCreated`).
const EXERCISE_EOF_COMMAND: &str = "exercise-eof";
/// The only action this consumer implements. Anything else is reported as
/// unsettled instead of being half-honored.
const EOF_ACTION_END_OUTPUT_STREAM: &str = "end-output-stream";
/// The terminal's own end-of-stream gesture: an interactive shell exits on
/// `exit`, which ends the PTY's output stream at its real end of file.
///
/// The line terminator is platform-specific. Windows ConPTY delivers Enter as CARRIAGE RETURN,
/// so a bare `\n` never completes the line there.
#[cfg(windows)]
const EOF_EXIT_INPUT: &[u8] = b"exit\r\n";
#[cfg(not(windows))]
const EOF_EXIT_INPUT: &[u8] = b"exit\n";
/// Bound on answering the QA-owned shell's own startup VT queries before ending its stream.
///
/// A freshly spawned shell sends its startup queries before its first prompt and BLOCKS until
/// they are answered. A production pane is answered by the app's native surface host, which
/// knows the session is starting up; this lane runs in the DAEMON, where no surface host exists,
/// so it must answer them itself. Measured without this: the shell's ONLY published bytes were
/// the 4-byte `ESC[6n`, with no prompt at all.
const EOF_STARTUP_QUERY_WINDOW_MS: u64 = 1_200;
const EOF_COLS: u16 = 80;
const EOF_ROWS: u16 = 24;
/// Bound on the private control watch for `exercise-eof`. The runner issues it
/// inside the native scenario, well after this daemon boots, so the window
/// covers the whole scenario while still stopping a stray watcher.
const EXERCISE_EOF_WINDOW_MS: u64 = 300_000;
/// Bounded wait for the stream's own end. The runner's budget for this step is
/// `BUDGETS.stageAttachListenerMs` (4s), so the settlement stays well inside it:
/// the shell's exit ends the stream, the hub close that follows is immediate,
/// and only a slow shell startup can approach this bound.
const EOF_STREAM_END_WINDOW_MS: u64 = 2_500;
/// Bounded wait for the hub's EOF path once the output pump has finished. The
/// pump removes the session hub before its own end-of-stream signal fires, so
/// this only bounds the pathological case.
const EOF_HUB_CLOSE_WINDOW_MS: u64 = 250;
/// Bounded settle of the lifecycle watcher's registry cleanup. It is evidence,
/// not the pass criterion (which is gated on the two real EOF observations), so
/// a slow cleanup is reported truthfully instead of being waited out. The
/// watcher removes the session in the same iteration that ends the pump, so this
/// only covers scheduling.
const EOF_REGISTRY_SETTLE_WINDOW_MS: u64 = 250;
/// Registry settle cadence: the private channel's own control cadence.
const EOF_REGISTRY_TICK_MS: u64 = 25;
const EOF_PRODUCER: &str = "terminal-qa-eof";
/// Shared sidecar of the cross-lane frame-evidence contract: the native lane is
/// its only writer, one correlated JSON object per real frame submission.
pub(crate) const FRAME_SUBMITTED_FILE: &str = "frame-submitted.jsonl";
/// Matches `QA_MARKER_SENTINEL` in `native_terminal/surface_host.rs` and
/// `MARKER_TEXT` in `scripts/lib/qa-scenarios/native-driver.mjs`.
const MARKER_TEXT: &str = "FERRYX_SPLIT_READY";
const OUTPUT_TAIL_LIMIT: usize = 8 * 1024;
/// The runner's presentation stage budget: a frame not submitted within it is
/// not presentation evidence, so the marker receipt settles with the truthful
/// negative instead of waiting past the runner's own bound.
const FRAME_SUBMISSION_WINDOW_MS: u64 = 2_000;
/// Matches `RELEASE_POLL_MS` in `ipc/qa_barrier.rs`: the private channel's own
/// control-watch cadence.
const CONTROL_POLL_MS: u64 = 25;
/// Bound on the private control watch for `trigger-suspension-check`; the
/// runner triggers it right after `fixture-setup`, so this only stops a stray
/// watcher from living forever.
const SUSPENSION_CHECK_WINDOW_MS: u64 = 60_000;
/// The observer never blocks or grows the PTY output pump: a saturated queue
/// drops observations, so a missing receipt stays a truthful failure.
const QA_EVENT_QUEUE: usize = 1024;
const MARKER_PRODUCER: &str = "terminal-qa-liveness";
const SUSPENSION_PRODUCER: &str = "terminal-qa-suspension";

#[derive(Default)]
struct OwnedPtyAccounting {
    created_total: AtomicU64,
    per_session: Mutex<HashMap<String, u64>>,
}

impl OwnedPtyAccounting {
    fn record_creation(&self, session_id: &str) -> u64 {
        self.created_total.fetch_add(1, Ordering::AcqRel);
        let mut counts = self
            .per_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = counts.entry(session_id.to_string()).or_insert(0);
        *entry += 1;
        *entry
    }

    fn creations_for(&self, session_id: &str) -> u64 {
        self.per_session
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_id)
            .copied()
            .unwrap_or(0)
    }

    fn created_total(&self) -> u64 {
        self.created_total.load(Ordering::Acquire)
    }
}

fn owned_pty_accounting() -> &'static OwnedPtyAccounting {
    static ACCOUNTING: OnceLock<OwnedPtyAccounting> = OnceLock::new();
    ACCOUNTING.get_or_init(OwnedPtyAccounting::default)
}

/// Real creations of local PTYs by this process: per session id and in total.
pub(crate) fn pty_created_counts(session_id: &str) -> (u64, u64) {
    let accounting = owned_pty_accounting();
    (
        accounting.creations_for(session_id),
        accounting.created_total(),
    )
}

/// Called from the successful local spawn path only: an adopted transfer is not
/// a creation, and nothing else may inflate the count.
pub(crate) fn record_owned_pty_creation(session_id: &str) {
    owned_pty_accounting().record_creation(session_id);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MarkerObservation {
    pub(crate) session_id: String,
    /// Hub sequence of the chunk whose bytes completed the occurrence.
    pub(crate) sequence: u64,
    /// The real observed output up to the end of the occurrence.
    pub(crate) output: String,
    pub(crate) output_byte_count: usize,
}

#[derive(Default)]
pub(crate) struct MarkerTracker {
    tails: HashMap<String, Vec<u8>>,
    submitted: HashMap<String, u64>,
}

impl MarkerTracker {
    /// Feeds one really published output chunk. Returns a new occurrence when
    /// the marker completed inside this chunk (or its chunk-boundary overlap);
    /// an occurrence already reported is never reported twice.
    pub(crate) fn observe_output(
        &mut self,
        session_id: &str,
        sequence: u64,
        bytes: &[u8],
    ) -> Option<MarkerObservation> {
        let marker = MARKER_TEXT.as_bytes();
        let overlap = marker.len() - 1;
        let tail = self.tails.entry(session_id.to_string()).or_default();
        let mut from = tail.len().saturating_sub(overlap);
        tail.extend_from_slice(bytes);
        if tail.len() > OUTPUT_TAIL_LIMIT {
            let excess = tail.len() - OUTPUT_TAIL_LIMIT;
            tail.drain(..excess);
            from = from.saturating_sub(excess);
        }
        let search_from = from.min(tail.len());
        let end = tail[search_from..]
            .windows(marker.len())
            .position(|window| window == marker)
            .map(|offset| search_from + offset + marker.len())?;
        Some(MarkerObservation {
            session_id: session_id.to_string(),
            sequence,
            output: String::from_utf8_lossy(&tail[..end]).into_owned(),
            output_byte_count: end,
        })
    }

    pub(crate) fn note_frame(&mut self, session_id: &str, covered_sequence: u64) {
        let entry = self.submitted.entry(session_id.to_string()).or_insert(0);
        *entry = (*entry).max(covered_sequence);
    }

    pub(crate) fn submitted_covering(&self, session_id: &str, sequence: u64) -> bool {
        self.submitted
            .get(session_id)
            .is_some_and(|covered| *covered >= sequence)
    }

    pub(crate) fn submitted_sequence(&self, session_id: &str) -> Option<u64> {
        self.submitted.get(session_id).copied()
    }
}

/// Runner field mapping for the marker receipt: `output`, `frameSubmitted` and
/// `ptyCreatedCount` are the fields the frozen runner asserts on, and
/// `frameSubmitted` is true only for real frame evidence covering this output.
pub(crate) fn marker_output_payload(
    observation: &MarkerObservation,
    frame_evidence: Option<u64>,
    pty_created_count: u64,
    pty_created_total: u64,
) -> Value {
    json!({
        "sessionId": observation.session_id,
        "output": observation.output,
        "outputSequence": observation.sequence,
        "outputByteCount": observation.output_byte_count,
        "markerText": MARKER_TEXT,
        "frameSubmitted": frame_evidence.is_some(),
        "frameCoveredSequence": frame_evidence,
        "frameEvidenceSource": FRAME_SUBMITTED_FILE,
        "frameEvidenceWindowMs": FRAME_SUBMISSION_WINDOW_MS,
        "ptyCreatedCount": pty_created_count,
        "ptyCreatedTotal": pty_created_total,
        "producerComponent": MARKER_PRODUCER,
    })
}

struct OutputObservation {
    session_id: String,
    sequence: u64,
    bytes: Arc<[u8]>,
}

struct QaLivenessState {
    events: mpsc::Sender<OutputObservation>,
}

static STATE: OnceLock<QaLivenessState> = OnceLock::new();

/// Installs the private channel for this process (idempotent, including the
/// pre-launch arm registration the runner requires) and starts the
/// terminal-layer producers. Called from `TerminalService::new`, which the
/// daemon builds inside its runtime; a normal launch or a test reaches no
/// channel and starts nothing.
pub(crate) fn start(service: TerminalService) {
    let channel = match qa_barrier::active_channel() {
        Some(channel) => channel,
        None => {
            let Ok(channel) = QaBarrierChannel::from_env() else {
                return;
            };
            qa_barrier::install(channel);
            let Some(channel) = qa_barrier::active_channel() else {
                return;
            };
            let (acked, rejected) = channel.scan_and_ack_arms();
            if !rejected.is_empty() {
                eprintln!("FERRYX_QA_ARM_REJECTED: daemon rejected prelaunch arms: {rejected:?}");
            }
            let _ = acked;
            channel
        }
    };
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let (events, receiver) = mpsc::channel(QA_EVENT_QUEUE);
    if STATE.set(QaLivenessState { events }).is_err() {
        return;
    }
    handle.spawn(observer_loop(Arc::clone(&channel), receiver));
    handle.spawn(suspension_check_watch(service.clone(), Arc::clone(&channel)));
    handle.spawn(exercise_eof_watch(service, channel));
}

/// Real output published to the session hub. Non-blocking by construction: the
/// PTY pump must never wait on the private QA channel.
pub(crate) fn observe_published_output(session_id: &str, sequence: u64, bytes: Arc<[u8]>) {
    let Some(state) = STATE.get() else {
        return;
    };
    let _ = state.events.try_send(OutputObservation {
        session_id: session_id.to_string(),
        sequence,
        bytes,
    });
}

fn barrier_dir() -> Option<PathBuf> {
    let dir = std::env::var("FERRYX_QA_BARRIER_DIR").ok()?;
    if dir.trim().is_empty() {
        return None;
    }
    Some(PathBuf::from(dir))
}

/// Highest sequence a really submitted frame covered for a session, from the
/// native lane's sidecar; only records correlated to this run/operation and
/// session are honored.
fn read_frame_submission(channel: &QaBarrierChannel, session_id: &str) -> Option<u64> {
    let operation_id = channel.operation_id()?;
    read_frame_submission_in(
        &barrier_dir()?,
        session_id,
        channel.run_id(),
        &operation_id,
    )
}

fn read_frame_submission_in(
    dir: &Path,
    session_id: &str,
    run_id: &str,
    operation_id: &str,
) -> Option<u64> {
    let text = std::fs::read_to_string(dir.join(FRAME_SUBMITTED_FILE)).ok()?;
    let mut highest: Option<u64> = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record.get("runId").and_then(Value::as_str) != Some(run_id) {
            continue;
        }
        if record.get("operationId").and_then(Value::as_str) != Some(operation_id) {
            continue;
        }
        if record.get("sessionId").and_then(Value::as_str) != Some(session_id) {
            continue;
        }
        if let Some(covered_sequence) = record.get("coveredSequence").and_then(Value::as_u64) {
            highest = Some(highest.map_or(covered_sequence, |seen: u64| seen.max(covered_sequence)));
        }
    }
    highest
}

async fn append_receipt(channel: &Arc<QaBarrierChannel>, name: &str, payload: Value) -> bool {
    let Some(operation_id) = channel.operation_id() else {
        return false;
    };
    let channel = Arc::clone(channel);
    let name = name.to_string();
    // The channel's append is synchronous file I/O, so it stays off the
    // runtime thread exactly like the native presentation producer does.
    tokio::task::spawn_blocking(move || channel.append_receipt(&name, &operation_id, payload))
        .await
        .is_ok()
}

async fn observer_loop(
    channel: Arc<QaBarrierChannel>,
    mut events: mpsc::Receiver<OutputObservation>,
) {
    let mut tracker = MarkerTracker::default();
    let mut queue: VecDeque<MarkerObservation> = VecDeque::new();
    loop {
        while queue.is_empty() {
            let Some(observation) = events.recv().await else {
                return;
            };
            record_observation(&mut tracker, &mut queue, observation);
        }
        let observation = queue.pop_front().expect("queue was just checked");
        let frame_evidence =
            await_frame_evidence(&channel, &mut tracker, &mut events, &mut queue, &observation)
                .await;
        let (pty_created_count, pty_created_total) = pty_created_counts(&observation.session_id);
        let payload = marker_output_payload(
            &observation,
            frame_evidence,
            pty_created_count,
            pty_created_total,
        );
        let _ = append_receipt(&channel, MARKER_OUTPUT_RECEIPT, payload).await;
    }
}

fn record_observation(
    tracker: &mut MarkerTracker,
    queue: &mut VecDeque<MarkerObservation>,
    observation: OutputObservation,
) {
    if let Some(occurrence) = tracker.observe_output(
        &observation.session_id,
        observation.sequence,
        &observation.bytes,
    ) {
        queue.push_back(occurrence);
    }
}

/// Bounded wait for the native lane's real frame evidence covering the
/// occurrence: `Some(covered)` only when a submitted frame really covers it.
async fn await_frame_evidence(
    channel: &Arc<QaBarrierChannel>,
    tracker: &mut MarkerTracker,
    events: &mut mpsc::Receiver<OutputObservation>,
    queue: &mut VecDeque<MarkerObservation>,
    observation: &MarkerObservation,
) -> Option<u64> {
    if tracker.submitted_covering(&observation.session_id, observation.sequence) {
        return tracker.submitted_sequence(&observation.session_id);
    }
    let deadline = tokio::time::Instant::now() + Duration::from_millis(FRAME_SUBMISSION_WINDOW_MS);
    let mut ticker = tokio::time::interval(Duration::from_millis(CONTROL_POLL_MS));
    loop {
        ticker.tick().await;
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        while let Ok(observation) = events.try_recv() {
            record_observation(tracker, queue, observation);
        }
        // The sidecar read is synchronous filesystem I/O; it runs on the blocking pool so this
        // 25 ms poll cadence never stalls a worker of the runtime that serves it.
        let polled_channel = Arc::clone(channel);
        let polled_session = observation.session_id.clone();
        let submitted = match crate::ipc::run_blocking(move || {
            Ok(read_frame_submission(&polled_channel, &polled_session))
        }).await {
            Ok(submitted) => submitted,
            // A blocking read that cannot complete is "no evidence observed yet", never a pass.
            Err(_) => None,
        };
        if let Some(covered) = submitted {
            tracker.note_frame(&observation.session_id, covered);
        }
        if tracker.submitted_covering(&observation.session_id, observation.sequence) {
            return tracker.submitted_sequence(&observation.session_id);
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct OwnedStopOutcome {
    pub(crate) session_id: String,
    pub(crate) suspend_pid: u32,
    pub(crate) resume_pid: u32,
    pub(crate) incarnation: String,
    pub(crate) actuated_at_unix_ms: u64,
    pub(crate) resumed: bool,
    pub(crate) resume_probe_state: &'static str,
    pub(crate) resume_identity_matches: bool,
    pub(crate) verified_actuation_receipt: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ExternalStopOutcome {
    pub(crate) session_id: Option<String>,
    pub(crate) pid: Option<u32>,
    pub(crate) incarnation: Option<String>,
    pub(crate) source: &'static str,
    pub(crate) probe_state: &'static str,
    pub(crate) auto_resumed: bool,
    pub(crate) reason: Option<&'static str>,
}

/// `None` means the ownership classification could not be observed at all.
pub(crate) fn probe_state_for(classification: Option<SuspensionSource>) -> &'static str {
    match classification {
        Some(SuspensionSource::FerryxOwned) => "owned",
        Some(SuspensionSource::External) => "stopped",
        Some(SuspensionSource::Unknown) => "running",
        None => "unobserved",
    }
}

/// Runner field mapping for the suspension receipt. The frozen runner asserts
/// `externallyStoppedProbeState`, `externallyStoppedAutoResumed`,
/// `ownedSuspendPid`/`ownedResumePid`, `ownedResumed` and
/// `verifiedActuationReceipt` on this single line.
pub(crate) fn suspension_receipt_payload(
    owned: &OwnedStopOutcome,
    external: &ExternalStopOutcome,
) -> Value {
    json!({
        "sessionId": owned.session_id,
        "ownedSessionId": owned.session_id,
        "ownedSource": "ferryx-owned",
        "ownedSuspendPid": owned.suspend_pid,
        "ownedResumePid": owned.resume_pid,
        "ownedResumed": owned.resumed,
        "ownedIncarnation": owned.incarnation,
        "ownedActuatedAtUnixMs": owned.actuated_at_unix_ms,
        "ownedResumeProbeState": owned.resume_probe_state,
        "ownedResumeIdentityMatches": owned.resume_identity_matches,
        "verifiedActuationReceipt": owned.verified_actuation_receipt,
        "externallyStoppedSessionId": external.session_id,
        "externallyStoppedPid": external.pid,
        "externallyStoppedIncarnation": external.incarnation,
        "externallyStoppedSource": external.source,
        "externallyStoppedProbeState": external.probe_state,
        "externallyStoppedAutoResumed": external.auto_resumed,
        "externalStopReason": external.reason,
        "ownershipClassification": {
            "owned": "ferryx-owned",
            "external": external.source,
        },
        "producerComponent": SUSPENSION_PRODUCER,
        "actuationSource": "terminal/service.rs::suspend_verified_session",
    })
}

async fn suspension_check_watch(service: TerminalService, channel: Arc<QaBarrierChannel>) {
    let Some(path) =
        barrier_dir().map(|dir| dir.join(format!("{SUSPENSION_CHECK_COMMAND}.request.json")))
    else {
        return;
    };
    let deadline = tokio::time::Instant::now() + Duration::from_millis(SUSPENSION_CHECK_WINDOW_MS);
    let mut ticker = tokio::time::interval(Duration::from_millis(CONTROL_POLL_MS));
    loop {
        ticker.tick().await;
        if tokio::time::Instant::now() >= deadline {
            return;
        }
        let Ok(text) = tokio::fs::read_to_string(&path).await else {
            continue;
        };
        let Ok(request) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if request.get("name").and_then(Value::as_str) != Some(SUSPENSION_CHECK_COMMAND) {
            continue;
        }
        if request.get("runId").and_then(Value::as_str) != Some(channel.run_id()) {
            continue;
        }
        let Some(operation_id) = channel.operation_id() else {
            continue;
        };
        if request.get("operationId").and_then(Value::as_str) != Some(operation_id.as_str()) {
            continue;
        }
        run_suspension_ownership_check(&service, &channel).await;
        return;
    }
}

/// Real suspension-ownership check against this daemon's own local PTYs.
///
/// The owned part goes through the production actuation path
/// (`TerminalService::suspend_verified_session` -> `resume_owned_session`), so a
/// receipt exists only after a successful, identity-verified OS actuation. The
/// external part observes a really stopped, unowned local session and runs the
/// product's auto-resume decision against it: it must refuse, and the process
/// must still be stopped afterwards.
async fn run_suspension_ownership_check(service: &TerminalService, channel: &Arc<QaBarrierChannel>) {
    let mut owned_candidate: Option<(String, SuspensionTarget)> = None;
    let mut external_candidate: Option<(String, SuspensionTarget)> = None;
    for session_id in service.pty_manager().list_sessions() {
        let Some(session) = service.get_session(&session_id) else {
            continue;
        };
        let Ok(target) = blocking_suspension_target(&session).await else {
            continue;
        };
        let classification = blocking_classification(&target).await;
        match classification {
            Ok(SuspensionSource::Unknown) if owned_candidate.is_none() => {
                owned_candidate = Some((session_id, target));
            }
            Ok(SuspensionSource::External) if external_candidate.is_none() => {
                external_candidate = Some((session_id, target));
            }
            _ => {}
        }
    }

    let Some((owned_session_id, owned_target)) = owned_candidate else {
        eprintln!(
            "FERRYX_QA_SUSPENSION_UNSETTLED: no running local PTY session with a verifiable suspension identity"
        );
        return;
    };

    let receipt = match service
        .suspend_verified_session(&owned_session_id, owned_target.clone())
        .await
    {
        Ok(receipt) => receipt,
        Err(error) => {
            eprintln!(
                "FERRYX_QA_SUSPENSION_UNSETTLED: owned actuation failed for '{owned_session_id}': {error}"
            );
            return;
        }
    };
    let verified = receipt.source == SuspensionSource::FerryxOwned
        && receipt.pid == owned_target.pid
        && receipt.incarnation == owned_target.incarnation;
    if !verified {
        eprintln!(
            "FERRYX_QA_SUSPENSION_UNSETTLED: actuation receipt did not verify the requested identity"
        );
        return;
    }

    if let Err(error) = service
        .resume_owned_session(&owned_session_id, owned_target.clone())
        .await
    {
        eprintln!(
            "FERRYX_QA_SUSPENSION_UNSETTLED: owned auto-resume failed for '{owned_session_id}': {error}"
        );
        return;
    }

    let (resume_pid, resume_identity_matches, resume_probe_state) =
        match service.get_session(&owned_session_id) {
            Some(session) => match blocking_suspension_target(&session).await {
                Ok(after) => {
                    let probe_state = probe_state_for(blocking_classification(&after).await.ok());
                    (after.pid, after == owned_target, probe_state)
                }
                Err(_) => (owned_target.pid, false, "unobserved"),
            },
            None => (owned_target.pid, false, "unobserved"),
        };

    let external = match external_candidate {
        Some((session_id, target)) => {
            let auto_resumed = blocking_auto_resume(&target).await.is_ok();
            let probe_state = probe_state_for(blocking_classification(&target).await.ok());
            ExternalStopOutcome {
                session_id: Some(session_id),
                pid: Some(target.pid),
                incarnation: Some(target.incarnation.clone()),
                source: "external",
                probe_state,
                auto_resumed,
                reason: None,
            }
        }
        None => ExternalStopOutcome {
            session_id: None,
            pid: None,
            incarnation: None,
            source: "unobserved",
            probe_state: "unobserved",
            auto_resumed: false,
            reason: Some("no-unowned-stopped-local-session"),
        },
    };

    let owned = OwnedStopOutcome {
        session_id: owned_session_id,
        suspend_pid: owned_target.pid,
        resume_pid,
        incarnation: owned_target.incarnation.clone(),
        actuated_at_unix_ms: receipt.actuated_at_unix_ms,
        resumed: resume_probe_state == "running",
        resume_probe_state,
        resume_identity_matches,
        verified_actuation_receipt: true,
    };
    let payload = suspension_receipt_payload(&owned, &external);
    if !append_receipt(channel, SUSPENSION_RECEIPT, payload).await {
        eprintln!("FERRYX_QA_SUSPENSION_UNSETTLED: no operation nonce to correlate the receipt");
    }
}

async fn blocking_suspension_target(session: &Arc<PtySession>) -> Result<SuspensionTarget, String> {
    let session = Arc::clone(session);
    crate::ipc::run_blocking(move || {
        session
            .suspension_target()
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .map_err(|error| error.to_string())
}

async fn blocking_classification(target: &SuspensionTarget) -> Result<SuspensionSource, String> {
    let target = target.clone();
    crate::ipc::run_blocking(move || {
        classify_stop_source(&target).map_err(crate::ipc::IpcError::internal)
    })
    .await
    .map_err(|error| error.to_string())
}

async fn blocking_auto_resume(target: &SuspensionTarget) -> Result<(), String> {
    let target = target.clone();
    crate::ipc::run_blocking(move || {
        auto_resume_suspension(&target, classify_stop_source, resume_owned)
            .map_err(crate::ipc::IpcError::internal)
    })
    .await
    .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------------------
// `exercise-eof` -> `eof-handled`
//
// The runner's step 4 ends one owned output stream and asserts two things about
// the product's own EOF handling: that the EOF was really observed
// (`eofObserved`) and that no replacement PTY was created (`newPtyCreated`).
//
// Both halves are product observations of the QA lane's OWN stream. The lane
// spawns a real local PTY through the production service path (never the
// runner's pane), subscribes to its output stream through the production attach
// path before the stream ends, and then lets the shell exit - so the PTY reader
// reaches its real end of file, the output pump finishes (the same
// end-of-stream signal `daemon/session_service.rs` consumes), the session hub is
// removed (the hub's EOF path: every real subscriber's stream closes) and the
// lifecycle watcher finalizes the natural exit. The receipt is settled from
// those observations only, and `newPtyCreated` is a measurement of this
// process's real PTY accounting, never a constant.

/// One real observation of the QA-owned output stream's end. Every field is a
/// value this process measured; nothing is inferred from the request.
#[derive(Debug, Clone)]
pub(crate) struct EofObservation {
    pub(crate) session_id: String,
    /// The hub's EOF path: the QA-owned subscription really reached `Closed`.
    pub(crate) stream_closed: bool,
    /// The PTY reader really reached the end of the stream. This is the
    /// session's own `reader_finished` flag, set only when its read loop ends,
    /// so a reader that is merely idle can never satisfy it.
    pub(crate) reader_finished: bool,
    pub(crate) hub_session_removed: bool,
    /// The lifecycle watcher's own settlement: the session left the registry.
    pub(crate) session_removed_from_registry: bool,
    pub(crate) pty_created_for_session_before: u64,
    pub(crate) pty_created_for_session_after: u64,
    pub(crate) pty_created_total_before: u64,
    pub(crate) pty_created_total_after: u64,
    pub(crate) live_sessions_before: usize,
    pub(crate) live_sessions_after: usize,
    pub(crate) eof_latency_ms: u64,
}

impl EofObservation {
    /// `eofObserved`: the stream really closed AND its reader really finished.
    /// Anything less is an unobserved EOF and settles no receipt.
    pub(crate) fn eof_observed(&self) -> bool {
        self.stream_closed && self.reader_finished
    }

    /// The runner's invariant, measured from real accounting: ending the stream
    /// must not have created a PTY - neither a replacement for the ended session
    /// nor any other new local PTY of this daemon. A replacement under the same
    /// id moves the per-session count; one under a new id moves the live-session
    /// census.
    pub(crate) fn new_pty_created(&self) -> bool {
        self.pty_created_for_session_after > self.pty_created_for_session_before
            || self.live_sessions_after > self.live_sessions_before
    }
}

/// Runner field mapping for the `eof-handled` receipt: `eofObserved` and
/// `newPtyCreated` are the fields the frozen runner asserts on, and every other
/// field is the evidence those two rest on.
pub(crate) fn eof_handled_payload(observation: &EofObservation) -> Value {
    let stream_closed_reason = if observation.stream_closed {
        json!("hub-subscription-closed")
    } else {
        Value::Null
    };
    json!({
        "sessionId": observation.session_id,
        "backendSessionId": observation.session_id,
        "action": EOF_ACTION_END_OUTPUT_STREAM,
        "stream": "owned-output-stream",
        "eofObserved": observation.eof_observed(),
        "streamClosed": observation.stream_closed,
        "streamClosedReason": stream_closed_reason,
        "readerFinished": observation.reader_finished,
        "hubSessionRemoved": observation.hub_session_removed,
        "sessionRemovedFromRegistry": observation.session_removed_from_registry,
        "newPtyCreated": observation.new_pty_created(),
        "ptyCreatedCount": observation.pty_created_for_session_after,
        "ptyCreatedCountBefore": observation.pty_created_for_session_before,
        "ptyCreatedTotalBefore": observation.pty_created_total_before,
        "ptyCreatedTotalAfter": observation.pty_created_total_after,
        "ptyCreationBaseline": "per-session creations of the QA-owned session plus this daemon's live-session census, measured after the QA-owned session existed and before its stream was ended",
        "liveSessionsBefore": observation.live_sessions_before,
        "liveSessionsAfter": observation.live_sessions_after,
        "eofLatencyMs": observation.eof_latency_ms,
        "stage": EOF_HANDLED_RECEIPT,
        "producerComponent": EOF_PRODUCER,
    })
}

/// A control that does not echo this run's and operation's nonces, or that is
/// addressed to another command, is never honored.
fn read_correlated_command(dir: &Path, name: &str, channel: &QaBarrierChannel) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join(format!("{name}.request.json"))).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    if value.get("name").and_then(Value::as_str) != Some(name) {
        return None;
    }
    if value.get("runId").and_then(Value::as_str) != Some(channel.run_id()) {
        return None;
    }
    let expected = channel.operation_id()?;
    if value.get("operationId").and_then(Value::as_str) != Some(expected.as_str()) {
        return None;
    }
    Some(value)
}

/// Bounded settle of the lifecycle watcher's registry cleanup. The receipt is
/// not gated on this (it is gated on the two real EOF observations), so a slow
/// cleanup is reported truthfully instead of being waited out.
async fn wait_for_registry_removal(service: &TerminalService, session_id: &str) -> bool {
    let deadline =
        tokio::time::Instant::now() + Duration::from_millis(EOF_REGISTRY_SETTLE_WINDOW_MS);
    let mut ticker = tokio::time::interval(Duration::from_millis(EOF_REGISTRY_TICK_MS));
    loop {
        if service.get_session(session_id).is_none() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        ticker.tick().await;
    }
}

async fn exercise_eof_watch(service: TerminalService, channel: Arc<QaBarrierChannel>) {
    let Some(dir) = barrier_dir() else {
        return;
    };
    exercise_eof_watch_in(dir, service, channel).await;
}

/// The consumer's whole path, with the control directory injected so the boot
/// decision and the consumer are testable without mutating the process env.
/// Only a correlated `end-output-stream` control is honored; a second issue of
/// the same control (same `issuedAt`) is not replayed.
async fn exercise_eof_watch_in(
    dir: PathBuf,
    service: TerminalService,
    channel: Arc<QaBarrierChannel>,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(EXERCISE_EOF_WINDOW_MS);
    let mut ticker = tokio::time::interval(Duration::from_millis(CONTROL_POLL_MS));
    let mut handled: Option<String> = None;
    loop {
        ticker.tick().await;
        if tokio::time::Instant::now() >= deadline {
            return;
        }
        let Some(request) = ({
            // Same rule as the frame-side read above: this poll runs on the runtime that serves
            // the 25 ms cadence, so the synchronous sidecar read goes to the blocking pool.
            let polled_dir = dir.clone();
            let polled_channel = Arc::clone(&channel);
            match crate::ipc::run_blocking(move || {
                Ok(read_correlated_command(&polled_dir, EXERCISE_EOF_COMMAND, &polled_channel))
            })
            .await
            {
                Ok(request) => request,
                // A blocking read that cannot complete is "no control observed yet", never a pass.
                Err(_) => None,
            }
        }) else {
            continue;
        };
        let issued = request
            .get("issuedAt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if handled.as_deref() == Some(issued.as_str()) {
            continue;
        }
        handled = Some(issued);
        if request.get("action").and_then(Value::as_str) != Some(EOF_ACTION_END_OUTPUT_STREAM) {
            eprintln!(
                "FERRYX_QA_EOF_UNSETTLED: unsupported exercise-eof action {:?}",
                request.get("action")
            );
            continue;
        }
        if let Err(error) = end_owned_output_stream(&service, &channel).await {
            // Through the daemon's own logger, not `eprintln!`: the daemon's stderr is a pipe
            // nothing drains and a failed write there PANICS the task, destroying the very
            // diagnosis this line carries. The runner archives this log (`archiveDaemonLog`).
            tracing::warn!(target: "ferryx_qa_liveness", "FERRYX_QA_EOF_UNSETTLED: {error}");
        }
    }
}

/// Ends ONE QA-owned output stream through the production paths and settles
/// `eof-handled` from what the product really did. The runner's own pane is
/// never touched: this lane owns the session it ends.
async fn end_owned_output_stream(
    service: &TerminalService,
    channel: &Arc<QaBarrierChannel>,
) -> Result<(), String> {
    // 1. A real local PTY owned by this QA lane, through the production spawn
    //    path. Its stream is the one the runner's step 4 ends.
    let (session_id, mut lifecycle) = service
        .spawn_shell(EOF_COLS, EOF_ROWS)
        .map_err(|error| format!("QA-owned session spawn failed: {error}"))?;
    let Some(session) = service.get_session(&session_id) else {
        return Err(format!(
            "QA-owned session '{session_id}' is not in this daemon's registry"
        ));
    };
    // 2. Subscribe through the production attach path BEFORE the stream ends, so
    //    the end is observed and never raced.
    let attachment = service
        .attach_with_sequence(&session_id, None)
        .map_err(|error| format!("QA-owned stream subscribe failed: {error}"))?;
    // The snapshot carries whatever the shell published before this subscription, so the startup
    // query is searched in BOTH it and the live receiver.
    let mut seen: Vec<u8> = attachment.snapshot.history;
    let mut stream = attachment.receiver;

    // Answer the shell's startup VT queries. A terminal answers these, and for its QA-owned
    // session this lane IS the terminal: the app's native surface host answers them for
    // production panes, but only for a session it was told is starting up, and this session is
    // spawned in the daemon where no surface host exists. Measured without this: the shell never
    // reached a prompt, never read the `exit` below, and no EOF was ever observed.
    let answers: [(&[u8], &[u8]); 2] = [
        (b"\x1b[6n", b"\x1b[1;1R"), // DSR cursor position
        (b"\x1b[c", b"\x1b[?1;2c"),  // primary device attributes
    ];
    let query_deadline =
        tokio::time::Instant::now() + Duration::from_millis(EOF_STARTUP_QUERY_WINDOW_MS);
    let mut answered = false;
    loop {
        for (query, answer) in answers {
            if seen.windows(query.len()).any(|window| window == query) {
                if service.write_input(&session_id, answer).is_ok() {
                    answered = true;
                }
                seen.clear();
            }
        }
        let now = tokio::time::Instant::now();
        if now >= query_deadline {
            break;
        }
        match tokio::time::timeout(query_deadline.saturating_duration_since(now), stream.recv()).await
        {
            Ok(Ok(chunk)) => {
                seen.extend_from_slice(&chunk.bytes);
                if answered {
                    break;
                }
            }
            Ok(Err(_)) => break,
            Err(_) => break,
        }
    }
    tracing::info!(target: "ferryx_qa_eof", answered, "startup query phase done");

    let (pty_created_for_session_before, pty_created_total_before) = pty_created_counts(&session_id);
    let live_sessions_before = service.list_sessions().len();

    // 3. End the stream: the shell exits, so the PTY reader reaches the end of
    //    its stream and stops.
    let started = tokio::time::Instant::now();
    if let Err(error) = service.write_input(&session_id, EOF_EXIT_INPUT) {
        let _ = service.close_session(&session_id).await;
        return Err(format!("ending the QA-owned stream failed: {error}"));
    }

    // 4. The product's own end-of-stream signal: the output pump holds this
    //    watch sender and drops it when the session's output stream is over.
    //    Either a change or the sender's drop means the pump finished; only the
    //    deadline means the stream is still open.
    let stream_ended = tokio::time::timeout(
        Duration::from_millis(EOF_STREAM_END_WINDOW_MS),
        lifecycle.changed(),
    )
    .await
    .is_ok();

    // 5. The hub's EOF path: the session hub was removed, so this real
    //    subscription reaches `Closed`. Real output before the end is drained,
    //    never skipped.
    let stream_closed = if stream_ended {
        matches!(
            tokio::time::timeout(Duration::from_millis(EOF_HUB_CLOSE_WINDOW_MS), async {
                loop {
                    match stream.recv().await {
                        Ok(_) => continue,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => return true,
                    }
                }
            })
            .await,
            Ok(true)
        )
    } else {
        false
    };

    // 6. The lifecycle watcher's own settlement of the natural exit.
    let reader_finished = session.is_reader_finished();
    let session_removed_from_registry = wait_for_registry_removal(service, &session_id).await;
    let hub_session_removed = !service.output_hub().has_session(&session_id);
    let eof_latency_ms = started.elapsed().as_millis() as u64;

    let (pty_created_for_session_after, pty_created_total_after) = pty_created_counts(&session_id);
    let live_sessions_after = service.list_sessions().len();
    let observation = EofObservation {
        session_id: session_id.clone(),
        stream_closed,
        reader_finished,
        hub_session_removed,
        session_removed_from_registry,
        pty_created_for_session_before,
        pty_created_for_session_after,
        pty_created_total_before,
        pty_created_total_after,
        live_sessions_before,
        live_sessions_after,
        eof_latency_ms,
    };
    if !observation.eof_observed() {
        // The truth is the negative: no EOF was observed, so no receipt is
        // settled (the runner's own bound then fails the scenario). The QA-owned
        // session is released so an unobserved stream leaves nothing behind.
        let _ = service.close_session(&session_id).await;
        return Err(format!(
            "no EOF was observed on the QA-owned stream (streamClosed={stream_closed}, readerFinished={reader_finished}, registryRemoved={session_removed_from_registry})"
        ));
    }
    let payload = eof_handled_payload(&observation);
    if !append_receipt(channel, EOF_HANDLED_RECEIPT, payload).await {
        return Err("no operation nonce to correlate the eof-handled receipt".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(session_id: &str, sequence: u64) -> MarkerObservation {
        MarkerObservation {
            session_id: session_id.to_string(),
            sequence,
            output: format!("prompt$ printf '{MARKER_TEXT}\\n'\r\n{MARKER_TEXT}\r\n"),
            output_byte_count: 64,
        }
    }

    #[test]
    fn marker_is_detected_across_chunk_boundaries_exactly_once() {
        let mut tracker = MarkerTracker::default();
        assert!(tracker
            .observe_output("pty-a", 7, b"prompt$ printf 'FERRYX_SPLIT_")
            .is_none());
        let occurrence = tracker
            .observe_output("pty-a", 8, b"READY\\n'\r\n")
            .expect("occurrence completes in the second chunk");
        assert_eq!(occurrence.session_id, "pty-a");
        assert_eq!(occurrence.sequence, 8);
        assert!(occurrence.output.contains(MARKER_TEXT));
        assert!(tracker
            .observe_output("pty-a", 9, b"more output\r\n")
            .is_none());
        let second = tracker
            .observe_output("pty-a", 10, format!("{MARKER_TEXT}\r\n").as_bytes())
            .expect("second occurrence");
        assert_eq!(second.sequence, 10);
    }

    #[test]
    fn marker_is_tracked_per_session() {
        let mut tracker = MarkerTracker::default();
        assert!(tracker.observe_output("pty-a", 1, b"nothing here").is_none());
        assert!(tracker
            .observe_output("pty-b", 2, format!("{MARKER_TEXT}\n").as_bytes())
            .is_some());
        assert!(tracker.observe_output("pty-a", 3, b"still nothing").is_none());
    }

    #[test]
    fn frame_evidence_must_cover_the_occurrence() {
        let mut tracker = MarkerTracker::default();
        tracker.note_frame("pty-a", 5);
        assert!(!tracker.submitted_covering("pty-a", 6));
        assert!(tracker.submitted_covering("pty-a", 5));
        assert!(!tracker.submitted_covering("pty-b", 5));
        tracker.note_frame("pty-a", 9);
        assert_eq!(tracker.submitted_sequence("pty-a"), Some(9));
        assert_eq!(tracker.submitted_sequence("pty-b"), None);
    }

    #[test]
    fn frame_sidecar_reader_honors_only_correlated_records() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let record = |run_id: &str, operation_id: &str, session_id: &str, covered: u64| {
            format!(
                "{}\n",
                json!({
                    "runId": run_id,
                    "operationId": operation_id,
                    "sessionId": session_id,
                    "coveredSequence": covered,
                    "producerComponent": "surface-host-gpu-render",
                })
            )
        };
        std::fs::write(
            dir.join(FRAME_SUBMITTED_FILE),
            [
                record("run-a", "op-a", "pty-a", 12),
                record("run-OTHER", "op-a", "pty-a", 99),
                record("run-a", "op-OTHER", "pty-a", 98),
                record("run-a", "op-a", "pty-b", 97),
                record("run-a", "op-a", "pty-a", 13),
                "not json".to_string(),
            ]
            .concat(),
        )
        .unwrap();
        assert_eq!(
            read_frame_submission_in(&dir, "pty-a", "run-a", "op-a"),
            Some(13)
        );
        assert_eq!(
            read_frame_submission_in(&dir, "pty-b", "run-a", "op-a"),
            Some(97)
        );
        assert_eq!(read_frame_submission_in(&dir, "pty-c", "run-a", "op-a"), None);
        assert_eq!(
            read_frame_submission_in(&dir, "pty-a", "run-b", "op-a"),
            None
        );
        assert_eq!(
            read_frame_submission_in(&dir, "pty-a", "run-a", "op-b"),
            None
        );
    }

    #[test]
    fn marker_payload_carries_runner_required_fields() {
        let observation = observation("pty-a", 12);
        let submitted = marker_output_payload(&observation, Some(12), 1, 3);
        assert_eq!(submitted["ptyCreatedCount"], json!(1));
        assert_eq!(submitted["ptyCreatedTotal"], json!(3));
        assert_eq!(submitted["frameSubmitted"], json!(true));
        assert_eq!(submitted["outputSequence"], json!(12));
        assert!(submitted["output"]
            .as_str()
            .is_some_and(|output| output.contains(MARKER_TEXT)));

        let unproven = marker_output_payload(&observation, None, 1, 3);
        assert_eq!(unproven["frameSubmitted"], json!(false));
        assert_eq!(unproven["frameCoveredSequence"], Value::Null);
    }

    #[test]
    fn owned_pty_accounting_counts_creations_per_session() {
        let accounting = OwnedPtyAccounting::default();
        assert_eq!(accounting.creations_for("pty-a"), 0);
        assert_eq!(accounting.record_creation("pty-a"), 1);
        assert_eq!(accounting.record_creation("pty-a"), 2);
        assert_eq!(accounting.record_creation("pty-b"), 1);
        assert_eq!(accounting.created_total(), 3);
        assert_eq!(accounting.creations_for("pty-a"), 2);
    }

    #[test]
    fn suspension_payload_carries_runner_invariants() {
        let owned = OwnedStopOutcome {
            session_id: "pty-owned".into(),
            suspend_pid: 4242,
            resume_pid: 4242,
            incarnation: "inc-1".into(),
            actuated_at_unix_ms: 1_000,
            resumed: true,
            resume_probe_state: "running",
            resume_identity_matches: true,
            verified_actuation_receipt: true,
        };
        let external = ExternalStopOutcome {
            session_id: Some("pty-external".into()),
            pid: Some(777),
            incarnation: Some("inc-ext".into()),
            source: "external",
            probe_state: "stopped",
            auto_resumed: false,
            reason: None,
        };
        let payload = suspension_receipt_payload(&owned, &external);
        assert_eq!(payload["ownedSuspendPid"], payload["ownedResumePid"]);
        assert_eq!(payload["ownedResumed"], json!(true));
        assert_eq!(payload["verifiedActuationReceipt"], json!(true));
        assert_eq!(payload["externallyStoppedProbeState"], json!("stopped"));
        assert_eq!(payload["externallyStoppedAutoResumed"], json!(false));
        assert_eq!(payload["externallyStoppedSource"], json!("external"));
        assert_eq!(
            payload["ownershipClassification"]["owned"],
            json!("ferryx-owned")
        );
        assert_eq!(
            payload["ownershipClassification"]["external"],
            json!("external")
        );
    }

    #[test]
    fn probe_states_map_real_classifications() {
        assert_eq!(probe_state_for(Some(SuspensionSource::FerryxOwned)), "owned");
        assert_eq!(probe_state_for(Some(SuspensionSource::External)), "stopped");
        assert_eq!(probe_state_for(Some(SuspensionSource::Unknown)), "running");
        assert_eq!(probe_state_for(None), "unobserved");
    }

    const EOF_TEST_RUN_ID: &str = "qa-run-eof";
    const EOF_TEST_OPERATION_ID: &str = "qa-op-eof";

    /// The channel's operation nonce comes from an armed barrier, exactly as it
    /// does in the runner's own pre-arm/ack sequence.
    fn armed_channel(dir: &Path) -> QaBarrierChannel {
        std::fs::write(
            dir.join(format!("{}.arm.json", crate::ipc::qa_barrier::WRITE_BARRIER)),
            serde_json::to_string_pretty(&json!({
                "name": crate::ipc::qa_barrier::WRITE_BARRIER,
                "runId": EOF_TEST_RUN_ID,
                "operationId": EOF_TEST_OPERATION_ID,
                "deadlineMs": 5_000,
            }))
            .unwrap(),
        )
        .unwrap();
        let channel = QaBarrierChannel::new(dir.to_path_buf(), EOF_TEST_RUN_ID.to_string());
        channel.scan_and_ack_arms();
        channel
    }

    fn eof_observation(session_id: &str) -> EofObservation {
        EofObservation {
            session_id: session_id.to_string(),
            stream_closed: true,
            reader_finished: true,
            hub_session_removed: true,
            session_removed_from_registry: true,
            pty_created_for_session_before: 1,
            pty_created_for_session_after: 1,
            pty_created_total_before: 7,
            pty_created_total_after: 7,
            live_sessions_before: 2,
            live_sessions_after: 1,
            eof_latency_ms: 42,
        }
    }

    // The runner's own field names, verbatim, each carrying the real
    // observation it rests on.
    #[test]
    fn eof_handled_payload_carries_the_runner_fields() {
        let payload = eof_handled_payload(&eof_observation("pty-eof"));
        assert_eq!(payload["eofObserved"], json!(true));
        assert_eq!(payload["newPtyCreated"], json!(false));
        assert_eq!(payload["streamClosed"], json!(true));
        assert_eq!(payload["streamClosedReason"], json!("hub-subscription-closed"));
        assert_eq!(payload["readerFinished"], json!(true));
        assert_eq!(payload["hubSessionRemoved"], json!(true));
        assert_eq!(payload["sessionRemovedFromRegistry"], json!(true));
        assert_eq!(payload["sessionId"], json!("pty-eof"));
        assert_eq!(payload["backendSessionId"], json!("pty-eof"));
        assert_eq!(payload["action"], json!(EOF_ACTION_END_OUTPUT_STREAM));
        assert_eq!(payload["ptyCreatedCount"], json!(1));
        assert_eq!(payload["ptyCreatedTotalBefore"], json!(7));
        assert_eq!(payload["ptyCreatedTotalAfter"], json!(7));
        assert_eq!(payload["liveSessionsBefore"], json!(2));
        assert_eq!(payload["liveSessionsAfter"], json!(1));
        assert_eq!(payload["eofLatencyMs"], json!(42));
        assert_eq!(payload["stage"], json!(EOF_HANDLED_RECEIPT));
        assert_eq!(payload["producerComponent"], json!(EOF_PRODUCER));
        assert!(payload["ptyCreationBaseline"]
            .as_str()
            .is_some_and(|basis| !basis.is_empty()));
    }

    // An unobserved EOF is never claimed, in either direction.
    #[test]
    fn eof_handled_payload_never_claims_an_unobserved_eof() {
        let mut stream_open = eof_observation("pty-eof");
        stream_open.stream_closed = false;
        let payload = eof_handled_payload(&stream_open);
        assert_eq!(payload["eofObserved"], json!(false));
        assert_eq!(payload["streamClosedReason"], Value::Null);

        let mut reader_open = eof_observation("pty-eof");
        reader_open.reader_finished = false;
        assert_eq!(eof_handled_payload(&reader_open)["eofObserved"], json!(false));
    }

    // `newPtyCreated` is a measurement, not a constant: a replacement under the
    // ended session's own id and any other new local PTY both report the truth.
    #[test]
    fn eof_handled_payload_reports_a_measured_new_pty() {
        let mut replacement = eof_observation("pty-eof");
        replacement.pty_created_for_session_after = 2;
        replacement.pty_created_total_after = 8;
        assert_eq!(eof_handled_payload(&replacement)["newPtyCreated"], json!(true));

        let mut new_pty = eof_observation("pty-eof");
        new_pty.live_sessions_after = 3;
        assert_eq!(eof_handled_payload(&new_pty)["newPtyCreated"], json!(true));

        let unchanged = eof_handled_payload(&eof_observation("pty-eof"));
        assert_eq!(unchanged["newPtyCreated"], json!(false));
    }

    // The consumer's whole path against the REAL product: a QA-owned local PTY
    // spawned through the production service, a real hub subscription opened
    // before the stream ends, the stream really ending, and the receipt the
    // runner awaits settled from those observations. The control directory is
    // injected, so this mutates no process env and cannot race a neighbour.
    #[test]
    fn exercise_eof_consumer_settles_from_the_real_stream_end() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = Arc::new(armed_channel(&dir));
        let mut receipts = channel.subscribe_receipts();
        let service = TerminalService::default();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let command = json!({
            "name": EXERCISE_EOF_COMMAND,
            "runId": EOF_TEST_RUN_ID,
            "operationId": EOF_TEST_OPERATION_ID,
            "issuedAt": "2026-10-04T00:00:00.000Z",
            "action": EOF_ACTION_END_OUTPUT_STREAM,
        });

        let settled = runtime.block_on(async {
            let watcher = tokio::spawn(exercise_eof_watch_in(
                dir.clone(),
                service.clone(),
                Arc::clone(&channel),
            ));
            std::fs::write(
                dir.join(format!("{EXERCISE_EOF_COMMAND}.request.json")),
                serde_json::to_vec(&command).unwrap(),
            )
            .unwrap();
            let settled = tokio::time::timeout(
                Duration::from_secs(30),
                QaBarrierChannel::await_receipt_event(&mut receipts, EOF_HANDLED_RECEIPT, 1),
            )
            .await;
            watcher.abort();
            settled
        });

        let payload = settled.expect("the consumer must settle from the real stream end");
        // The runner's correlation gate (`correlateReceipt`) and its two
        // assertions.
        assert_eq!(payload["runId"], json!(EOF_TEST_RUN_ID));
        assert_eq!(payload["operationId"], json!(EOF_TEST_OPERATION_ID));
        assert_eq!(payload["producer"], json!(crate::ipc::qa_barrier::PRODUCER_ID));
        assert_eq!(payload["eofObserved"], json!(true), "{payload}");
        assert_eq!(payload["newPtyCreated"], json!(false), "{payload}");
        // ... and the observations they rest on, all of them real.
        assert_eq!(payload["streamClosed"], json!(true), "{payload}");
        assert_eq!(payload["readerFinished"], json!(true), "{payload}");
        assert_eq!(payload["hubSessionRemoved"], json!(true), "{payload}");
        assert_eq!(payload["sessionRemovedFromRegistry"], json!(true), "{payload}");
        assert_eq!(payload["ptyCreatedCount"], json!(1), "{payload}");
        assert_eq!(payload["liveSessionsBefore"], json!(1), "{payload}");
        assert_eq!(payload["liveSessionsAfter"], json!(0), "{payload}");
        assert!(payload["eofLatencyMs"].as_u64().is_some(), "{payload}");

        // The ended stream belonged to this lane's own session, and that
        // session is really gone from the daemon afterwards.
        let session_id = payload["sessionId"]
            .as_str()
            .expect("the receipt names the session whose stream it ended");
        assert!(!session_id.is_empty());
        assert!(service.get_session(session_id).is_none());
        assert!(!service.output_hub().has_session(session_id));
    }

    // A control that does not echo this run's nonces, this operation's nonce or
    // this command's own name is never honored.
    #[test]
    fn eof_control_must_echo_this_run_and_operation() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("barriers");
        std::fs::create_dir_all(&dir).unwrap();
        let channel = armed_channel(&dir);
        let path = dir.join(format!("{EXERCISE_EOF_COMMAND}.request.json"));
        let write = |run_id: &str, operation_id: &str| {
            std::fs::write(
                &path,
                serde_json::to_vec(&json!({
                    "name": EXERCISE_EOF_COMMAND,
                    "runId": run_id,
                    "operationId": operation_id,
                    "action": EOF_ACTION_END_OUTPUT_STREAM,
                }))
                .unwrap(),
            )
            .unwrap();
        };

        write("qa-run-OTHER", EOF_TEST_OPERATION_ID);
        assert!(read_correlated_command(&dir, EXERCISE_EOF_COMMAND, &channel).is_none());
        write(EOF_TEST_RUN_ID, "qa-op-OTHER");
        assert!(read_correlated_command(&dir, EXERCISE_EOF_COMMAND, &channel).is_none());
        std::fs::write(
            dir.join(format!("{SUSPENSION_CHECK_COMMAND}.request.json")),
            serde_json::to_vec(&json!({
                "name": SUSPENSION_CHECK_COMMAND,
                "runId": EOF_TEST_RUN_ID,
                "operationId": EOF_TEST_OPERATION_ID,
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(read_correlated_command(&dir, EXERCISE_EOF_COMMAND, &channel).is_none());
        write(EOF_TEST_RUN_ID, EOF_TEST_OPERATION_ID);
        let correlated = read_correlated_command(&dir, EXERCISE_EOF_COMMAND, &channel)
            .expect("a fully correlated control is honored");
        assert_eq!(correlated["action"], json!(EOF_ACTION_END_OUTPUT_STREAM));
    }
}
