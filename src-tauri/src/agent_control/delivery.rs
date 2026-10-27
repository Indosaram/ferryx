//! Confirmed send: a supervisor instruction counts as delivered only when the observer sees the
//! pane start a turn after the bytes were accepted.
//!
//! `AgentControlOp::Send` ends at `DeliveryStage::Accepted`: the writer took the bytes. That is
//! not the agent having started. [`send_confirmed`] takes the observer cursor BEFORE the send,
//! runs the existing Send handler once, then waits on the observer log for a reading of the same
//! session, newer than the cursor, that is Working, Blocked, or carries a prompt. Nothing here
//! ever sends twice: an unconfirmed send is `Uncertain`, and the caller must not resend it.
//!
//! A pane that was already Working or Blocked before the send needs more: its next Working
//! reading proves nothing about this send, so the reading must also carry a screen revision
//! different from the pre-send one (see [`PreSend`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use super::events::{self, AgentEventBus, AgentObservation, AgentReplay, WaitHit};
use super::{AgentControlOp, AgentControlState};
use crate::remote::RemoteGatewayState;
use crate::remote::server::agent_control_service;
use crate::scoped_contracts::{ScopeError, ScopeErrorCode};

/// Env override for the turn-start deadline, in milliseconds.
pub const TURN_START_ENV: &str = "FERRYX_SUPERVISOR_TURN_START_MS";
pub const TURN_START_DEFAULT_MS: u64 = 20_000;
pub const TURN_START_MIN_MS: u64 = 2_000;
pub const TURN_START_MAX_MS: u64 = 120_000;

/// Code for an accepted send whose turn start was never observed.
pub const TURN_NOT_OBSERVED: &str = "TURN_NOT_OBSERVED";

/// What a confirmed send learned. Variant names mirror the hub-side `SendOutcome`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    /// The pane went Working or Blocked, or showed a prompt, after the send.
    Started,
    /// The bytes may have reached the pane but no turn start was seen. Never resend.
    Uncertain { code: String },
    /// The send was refused; the handler typed nothing.
    Refused { code: String },
}

/// Parse the env override: unset or unreadable is the default, anything else is clamped.
pub fn parse_turn_start_ms(raw: Option<&str>) -> Duration {
    let ms = raw
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|ms| ms.clamp(TURN_START_MIN_MS, TURN_START_MAX_MS))
        .unwrap_or(TURN_START_DEFAULT_MS);
    Duration::from_millis(ms)
}

/// The deadline a supervisor send waits for a turn start, from `FERRYX_SUPERVISOR_TURN_START_MS`.
pub fn turn_start_deadline() -> Duration {
    parse_turn_start_ms(std::env::var(TURN_START_ENV).ok().as_deref())
}

/// The wire string of a scope code (SCREAMING_SNAKE_CASE, with the explicit
/// `OPERATION_OUTCOME_UNKNOWN` rename).
fn wire_code(code: ScopeErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{code:?}"))
}

fn refusal_outcome(error: &ScopeError) -> SendOutcome {
    let code = wire_code(error.code);
    if error.code == ScopeErrorCode::OperationOutcomeUnknown
        || crate::remote::reference_chat::types::reference_is_outcome_unknown(&code)
    {
        SendOutcome::Uncertain { code }
    } else {
        SendOutcome::Refused { code }
    }
}

/// The pane's state as recorded just before the send.
///
/// When the latest pre-send reading was already Working or Blocked, `busy_revision` holds its
/// screen revision and a turn start additionally needs a reading whose screen revision differs
/// from it (on top of `sequence > cursor`). An idle or unknown pane needs only the newer
/// Working/Blocked/prompt reading.
#[derive(Debug, Clone, Default)]
struct PreSend {
    busy_revision: Option<Option<String>>,
}

impl PreSend {
    fn of(bus: &AgentEventBus, session_id: &str) -> Self {
        let busy_revision = bus
            .latest(session_id)
            .filter(|observation| {
                matches!(
                    observation.state,
                    AgentControlState::Working | AgentControlState::Blocked
                )
            })
            .map(|observation| observation.screen_revision);
        Self { busy_revision }
    }

    fn screen_moved(&self, observation: &AgentObservation) -> bool {
        match &self.busy_revision {
            Some(before) => observation.screen_revision != *before,
            None => true,
        }
    }
}

fn starts_turn(
    observation: &AgentObservation,
    session_id: &str,
    cursor: u64,
    pre: &PreSend,
) -> bool {
    observation.session_id == session_id
        && observation.sequence > cursor
        && (matches!(
            observation.state,
            AgentControlState::Working | AgentControlState::Blocked
        ) || observation.prompt_id.is_some())
        && pre.screen_moved(observation)
}

/// Newest sequence issued or logged ([`AgentEventBus::head`]). Not the log revision alone: a
/// reading can hold its sequence before it is logged, and must not count as newer than a cursor
/// taken before the send.
fn cursor_now(bus: &AgentEventBus) -> u64 {
    bus.head()
}

/// Whether any reading still in the log after `cursor` starts a turn for `session_id`.
///
/// `wait_for` hands back only the newest matching reading, so a Working that an Idle followed
/// before the wait looked would otherwise be skipped.
fn started_in_log(bus: &AgentEventBus, session_id: &str, cursor: u64, pre: &PreSend) -> bool {
    match bus.replay(cursor) {
        AgentReplay::Events { events, .. } => events
            .iter()
            .any(|observation| starts_turn(observation, session_id, cursor, pre)),
        AgentReplay::Gap { snapshot, .. } => snapshot
            .iter()
            .any(|observation| starts_turn(observation, session_id, cursor, pre)),
    }
}

/// Send `text` once through the existing Send handler, then wait up to `deadline` for the pane
/// to start a turn. Never retries the send.
pub async fn send_confirmed(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    text: String,
    request_id: String,
    deadline: Duration,
) -> SendOutcome {
    let bus = Arc::clone(&state.agent_events);
    // Before the send, so a reading the send causes is always newer than the cursor.
    let mut cursor = cursor_now(&bus);
    let pre = PreSend::of(&bus, session_id);
    let op = AgentControlOp::Send {
        session_id: session_id.to_string(),
        text,
        request_id: Some(request_id),
    };
    if let Err(error) = crate::remote::server::agent_control_service::handle(state, op).await {
        return refusal_outcome(&error);
    }

    let until = tokio::time::Instant::now() + deadline;
    loop {
        let remaining = until.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match events::wait_for(&bus, session_id, "any", cursor, None, remaining).await {
            None => break,
            Some(WaitHit::Observation(observation)) => {
                if starts_turn(&observation, session_id, cursor, &pre)
                    || started_in_log(&bus, session_id, cursor, &pre)
                {
                    return SendOutcome::Started;
                }
                cursor = cursor.max(observation.sequence);
            }
            Some(WaitHit::Gap {
                snapshot,
                after_sequence,
            }) => {
                // The cursor fell out of the log. The latest reading per session still counts
                // when it is newer than the cursor; otherwise keep waiting from the log head.
                if snapshot
                    .iter()
                    .any(|observation| starts_turn(observation, session_id, cursor, &pre))
                {
                    return SendOutcome::Started;
                }
                cursor = cursor.max(after_sequence);
            }
        }
    }
    SendOutcome::Uncertain {
        code: TURN_NOT_OBSERVED.to_string(),
    }
}

/// Code for a turn start that cannot be attributed to the delivered request: other input
/// reached the pane after the send's cursor (another request, a phone chat message, a person
/// typing), so the start may be theirs.
pub const FOREIGN_INPUT: &str = "FOREIGN_INPUT";

/// What a delivery learned once its bytes were typed. A refusal (nothing typed) and an
/// unknown outcome are errors of the send itself, exactly as for a plain send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum DeliveryReport {
    /// The pane started a turn that only this request can have caused. `run_id` names the
    /// observer reading that started it ([`ferryx_hub::agent_control::pty_run_id`]).
    Started {
        request_id: String,
        session_id: String,
        run_id: String,
    },
    /// The bytes were typed but no start attributable to this request was seen. Never resend.
    Accepted {
        request_id: String,
        session_id: String,
        code: String,
    },
    /// Nothing was typed: the pane was not observed ready (idle, with no prompt) before the
    /// send, so its next turn start could not be attributed to this request.
    NotReady {
        request_id: String,
        session_id: String,
        code: String,
    },
}

/// How long a delivery waits for the bound pane to be observed ready before it types nothing.
pub const PANE_READY_WAIT: Duration = Duration::from_secs(10);

/// How long an input stays able to start a turn the observer has not published yet: the
/// observer's trailing debounce plus its quiet timer (`daemon::agent_observer::ObserverTiming`).
fn input_settle_horizon() -> Duration {
    let timing = crate::daemon::agent_observer::ObserverTiming::default();
    timing.debounce + timing.quiet
}

/// Whether the latest reading of `session_id` is idle with no prompt AND covers the newest
/// input that reached the pane: no turn is running, so the pane's next turn start can be
/// attributed to what is typed now. A pane without readings (a fresh pane still booting) is
/// not ready.
///
/// Readings trail the pane by the observer's debounce and quiet timer, so an idle reading
/// alone can be stale: input of another request (a phone chat message, an agent-control Send,
/// raw keys) written just before may have started a turn the observer has not published. The
/// pane counts as ready only when its idle reading was allocated after that input's mark and
/// the input is older than the observer's settle horizon
/// ([`AgentEventBus::input_observed`]).
fn pane_ready(bus: &AgentEventBus, session_id: &str) -> bool {
    bus.latest(session_id).is_some_and(|observation| {
        observation.state == AgentControlState::Idle
            && observation.prompt_id.is_none()
            && bus.input_observed(session_id, observation.sequence, input_settle_horizon())
    })
}

/// Waits up to `deadline` for `session_id` to be observed ready. It subscribes before it reads
/// the latest reading, so a reading published in between is never missed, and it also wakes
/// when the newest input of the pane passes the observer's settle horizon.
async fn wait_until_ready(bus: &AgentEventBus, session_id: &str, deadline: Duration) -> bool {
    let mut receiver = bus.subscribe();
    let until = tokio::time::Instant::now() + deadline;
    loop {
        if pane_ready(bus, session_id) {
            return true;
        }
        let now = tokio::time::Instant::now();
        if now >= until {
            return false;
        }
        let wake = bus
            .input_settles_at(session_id, input_settle_horizon())
            .filter(|at| *at > now)
            .map_or(until, |at| at.min(until));
        if let Ok(Err(broadcast::error::RecvError::Closed)) =
            tokio::time::timeout_at(wake, receiver.recv()).await
        {
            return pane_ready(bus, session_id);
        }
    }
}

fn not_ready(request_id: String, session_id: &str) -> DeliveryReport {
    DeliveryReport::NotReady {
        request_id,
        session_id: session_id.to_string(),
        code: ferryx_hub::agent_control::PANE_NOT_READY.to_string(),
    }
}

/// The start wait a delivery asked for: 0 is the configured default, anything else is clamped
/// like the env override.
pub fn observe_deadline(observe_ms: u64) -> Duration {
    if observe_ms == 0 {
        return turn_start_deadline();
    }
    Duration::from_millis(observe_ms.clamp(TURN_START_MIN_MS, TURN_START_MAX_MS))
}

/// The earliest reading after `after` that starts a turn for `session_id`.
fn first_start(
    readings: impl IntoIterator<Item = AgentObservation>,
    session_id: &str,
    after: u64,
    pre: &PreSend,
) -> Option<AgentObservation> {
    readings
        .into_iter()
        .filter(|observation| starts_turn(observation, session_id, after, pre))
        .min_by_key(|observation| observation.sequence)
}

fn logged_start(bus: &AgentEventBus, session_id: &str, after: u64, pre: &PreSend) -> Option<AgentObservation> {
    match bus.replay(after) {
        AgentReplay::Events { events, .. } => first_start(events, session_id, after, pre),
        AgentReplay::Gap { snapshot, .. } => first_start(snapshot, session_id, after, pre),
    }
}

/// Type `text` once into `session_id`, then attribute the pane's next turn start to
/// `request_id` only when the cause cannot be anything else.
///
/// Nothing is typed unless the pane is observed ready: its latest reading idle with no prompt,
/// newer than the pane's newest input and taken once that input is older than the observer's
/// settle horizon, waited for up to [`PANE_READY_WAIT`] (subscribe first) and checked again
/// under the pane's input step after the blocked-prompt probe, right before the first byte.
/// Otherwise the report is [`DeliveryReport::NotReady`]. A busy pane's next reading belongs to
/// its running turn, a fresh pane's to its boot, and a pane whose newest input is not yet
/// covered by an idle reading may be starting that input's turn, so none can carry this
/// request's start.
///
/// The observer cursor and the input cursor are both taken BEFORE the send (subscribe before
/// trigger). A start is a reading of this session newer than that cursor that is Working,
/// Blocked or carries a prompt. It is attributed only when no other input reached this pane
/// since the cursor ([`AgentEventBus::foreign_input_since`]): every writer marks the ledger
/// right before its bytes go out, the input queue's writer with its request id (another
/// request, a phone chat message) and each raw write path without one (a person typing or
/// pasting). Activity of another session, or of this pane caused by someone else, never starts
/// this request's run. Its run id names the start reading and this daemon's observer epoch.
/// Never retries the send.
///
/// Known limit: activity the agent starts on its own between the cursor and the Enter (a hook,
/// a background notification) carries no input mark and is still attributed to this request;
/// so is the turn of an earlier input whose agent showed an idle screen after that input and
/// then stayed silent for the whole settle horizon before it started.
pub async fn deliver(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    text: String,
    request_id: String,
    deadline: Duration,
    authorize: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Result<DeliveryReport, ScopeError> {
    let bus = Arc::clone(&state.agent_events);
    if !wait_until_ready(&bus, session_id, PANE_READY_WAIT).await {
        return Ok(not_ready(request_id, session_id));
    }
    let origin = cursor_now(&bus);
    let inputs = bus.input_cursor();
    let pre = PreSend::of(&bus, session_id);
    // Asked again under the pane's input step: input queued ahead of this one may have started
    // a turn while it waited.
    let unready = Arc::new(AtomicBool::new(false));
    let ready: Arc<dyn Fn() -> bool + Send + Sync> = {
        let bus = Arc::clone(&bus);
        let session_id = session_id.to_string();
        let unready = Arc::clone(&unready);
        Arc::new(move || {
            let ready = pane_ready(&bus, &session_id);
            unready.store(!ready, Ordering::SeqCst);
            ready
        })
    };
    match agent_control_service::send_text(
        state,
        session_id.to_string(),
        text,
        Some(request_id.clone()),
        authorize,
        Some(ready),
    )
    .await
    {
        Ok(_) => {}
        // Refused under the step because the pane was no longer ready: nothing was typed.
        Err(error)
            if error.code == ScopeErrorCode::ControlConflict && unready.load(Ordering::SeqCst) =>
        {
            return Ok(not_ready(request_id, session_id));
        }
        Err(error) => return Err(error),
    }

    let attribute = |observation: &AgentObservation| {
        if bus.foreign_input_since(session_id, inputs, &request_id) {
            DeliveryReport::Accepted {
                request_id: request_id.clone(),
                session_id: session_id.to_string(),
                code: FOREIGN_INPUT.to_string(),
            }
        } else {
            DeliveryReport::Started {
                request_id: request_id.clone(),
                session_id: session_id.to_string(),
                run_id: ferryx_hub::agent_control::pty_run_id(
                    session_id,
                    bus.instance(),
                    observation.sequence,
                ),
            }
        }
    };
    let mut cursor = origin;
    let until = tokio::time::Instant::now() + deadline;
    loop {
        // Every reading since the send, including one published while the bytes were typed.
        if let Some(start) = logged_start(&bus, session_id, origin, &pre) {
            return Ok(attribute(&start));
        }
        let remaining = until.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match events::wait_for(&bus, session_id, "any", cursor, None, remaining).await {
            None => break,
            Some(WaitHit::Observation(observation)) => {
                if starts_turn(&observation, session_id, origin, &pre) {
                    return Ok(attribute(&observation));
                }
                cursor = cursor.max(observation.sequence);
            }
            Some(WaitHit::Gap {
                snapshot,
                after_sequence,
            }) => {
                if let Some(start) = first_start(snapshot, session_id, origin, &pre) {
                    return Ok(attribute(&start));
                }
                cursor = cursor.max(after_sequence);
            }
        }
    }
    Ok(DeliveryReport::Accepted {
        request_id,
        session_id: session_id.to_string(),
        code: TURN_NOT_OBSERVED.to_string(),
    })
}

/// Code for a delivery the target answered as typed but without a readable start receipt.
pub const START_RECEIPT_MISSING: &str = "START_RECEIPT_MISSING";

/// The hub's view of one delivery on this machine: a start receipt, a typed send without one,
/// a pane that was not ready (nothing typed), or the send's own failure. A failure is a
/// refusal only for a code that proves nothing was typed
/// ([`ferryx_hub::agent_control::Delivery::from_error_code`]); any other is uncertain.
pub fn hub_delivery(result: Result<DeliveryReport, ScopeError>) -> ferryx_hub::agent_control::Delivery {
    use ferryx_hub::agent_control::{Delivery, StartReceipt};
    match result {
        Ok(DeliveryReport::Started {
            request_id,
            session_id,
            run_id,
        }) => Delivery::Started(StartReceipt {
            request_id,
            session_id,
            run_id,
        }),
        Ok(DeliveryReport::Accepted { code, .. }) => Delivery::Uncertain {
            accepted: true,
            code,
        },
        Ok(DeliveryReport::NotReady { code, .. }) => Delivery::from_error_code(Some(code.as_str())),
        Err(error) => Delivery::from_error_code(Some(wire_code(error.code).as_str())),
    }
}

/// The hub's view of a successful `Deliver` reply (`{delivery}`) from a paired host. The send
/// was typed, so a reply without a readable report is accepted without a start, never a start.
pub fn hub_delivery_from_reply(result: &serde_json::Value) -> ferryx_hub::agent_control::Delivery {
    match result
        .get("delivery")
        .cloned()
        .map(serde_json::from_value::<DeliveryReport>)
    {
        Some(Ok(report)) => hub_delivery(Ok(report)),
        _ => ferryx_hub::agent_control::Delivery::Uncertain {
            accepted: true,
            code: START_RECEIPT_MISSING.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_control::events::ObservationDraft;
    use crate::agent_control::{AgentStateSource, AgentTier};
    use crate::remote::backend::{RemoteSessionBackend, RemoteSessionDetails};
    use crate::remote::reference_chat::input::REFERENCE_SUBMIT_ENTER;
    use futures_util::future::BoxFuture;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};
    use tokio::sync::oneshot;

    const DEADLINE: Duration = Duration::from_secs(20);

    /// One shell pane. Writing a submit's Enter publishes `on_enter` to the observer bus, which
    /// stands in for the observer seeing the pane react to the send, unless `skip_enters` says
    /// to let that many Enters pass silently first. Every write is recorded. When `gate` is
    /// set, the next write signals its first half and does not complete until the second fires.
    struct Pane {
        session_id: String,
        writes: Mutex<Vec<Vec<u8>>>,
        write_attempts: AtomicUsize,
        fail_writes: AtomicBool,
        bus: OnceLock<Arc<AgentEventBus>>,
        on_enter: Mutex<Vec<ObservationDraft>>,
        skip_enters: AtomicUsize,
        gate: Mutex<Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>>,
    }

    impl RemoteSessionBackend for Pane {
        fn list_sessions(&self) -> BoxFuture<'_, Vec<String>> {
            Box::pin(async move { vec![self.session_id.clone()] })
        }
        fn describe_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> BoxFuture<'a, Result<RemoteSessionDetails, String>> {
            Box::pin(async move {
                if session_id != self.session_id {
                    return Err("unknown session".to_string());
                }
                Ok(RemoteSessionDetails {
                    session_id: self.session_id.clone(),
                    workspace_id: None,
                    worktree_label: None,
                    worktree_path: None,
                    running: true,
                    cols: 80,
                    rows: 24,
                })
            })
        }
        fn attach_with_sequence<'a>(
            &'a self,
            _session_id: &'a str,
            _after_sequence: Option<u64>,
        ) -> BoxFuture<'a, Result<crate::terminal::SessionAttachment, String>> {
            Box::pin(async move {
                let screen = b"$ ".to_vec();
                let (sender, receiver) = tokio::sync::broadcast::channel(1);
                std::mem::forget(sender);
                Ok(crate::terminal::SessionAttachment {
                    snapshot: crate::terminal::AttachmentSnapshot {
                        session_id: self.session_id.clone(),
                        history_start_sequence: Some(1),
                        history_end_sequence: Some(1),
                        history: screen.clone(),
                        history_segments: vec![crate::terminal::HistorySegment {
                            cols: Some(80),
                            rows: Some(24),
                            bytes: screen,
                        }],
                        gap: None,
                        history_incomplete: false,
                    },
                    receiver,
                })
            })
        }
        fn write_input<'a>(
            &'a self,
            _session_id: &'a str,
            data: &'a [u8],
        ) -> BoxFuture<'a, Result<(), String>> {
            self.write_attempts.fetch_add(1, Ordering::SeqCst);
            if self.fail_writes.load(Ordering::SeqCst) {
                return Box::pin(async { Err("the write failed".to_string()) });
            }
            self.writes.lock().expect("writes").push(data.to_vec());
            if data == REFERENCE_SUBMIT_ENTER && self.skip_enters.load(Ordering::SeqCst) > 0 {
                self.skip_enters.fetch_sub(1, Ordering::SeqCst);
            } else if data == REFERENCE_SUBMIT_ENTER {
                let bus = self.bus.get().expect("bus attached");
                for draft in self.on_enter.lock().expect("on_enter").drain(..) {
                    bus.publish(draft);
                }
            }
            if let Some((entered, release)) = self.gate.lock().expect("gate").take() {
                return Box::pin(async move {
                    let _ = entered.send(());
                    let _ = release.await;
                    Ok(())
                });
            }
            Box::pin(async { Ok(()) })
        }
        fn resize<'a>(
            &'a self,
            _session_id: &'a str,
            _cols: u16,
            _rows: u16,
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Err("this pane never resizes".to_string()) })
        }
        fn signal<'a>(
            &'a self,
            _session_id: &'a str,
            _signal: crate::terminal::TerminalSignal,
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Err("this pane is never signalled".to_string()) })
        }
    }

    fn pane(session_id: &str, on_enter: Vec<ObservationDraft>) -> (Arc<Pane>, Arc<RemoteGatewayState>) {
        let backend = Arc::new(Pane {
            session_id: session_id.to_string(),
            writes: Mutex::new(Vec::new()),
            write_attempts: AtomicUsize::new(0),
            fail_writes: AtomicBool::new(false),
            bus: OnceLock::new(),
            on_enter: Mutex::new(on_enter),
            skip_enters: AtomicUsize::new(0),
            gate: Mutex::new(None),
        });
        let state = Arc::new(RemoteGatewayState::new_with_backend(
            Arc::clone(&backend) as Arc<dyn RemoteSessionBackend>,
            crate::worktree::registry::WorkspaceRegistry::new(),
        ));
        assert!(backend.bus.set(Arc::clone(&state.agent_events)).is_ok());
        (backend, state)
    }

    fn draft(session_id: &str, state: AgentControlState) -> ObservationDraft {
        ObservationDraft {
            session_id: session_id.to_string(),
            state,
            state_source: AgentStateSource::Screen,
            tier: AgentTier::Screen,
            prompt_id: None,
            kind: None,
            screen_revision: Some(format!("rev-{state:?}")),
            workspace_id: None,
        }
    }

    /// One submit is its body write plus the separately written Enter.
    fn assert_one_submit(backend: &Pane) {
        let writes = backend.writes.lock().expect("writes").clone();
        assert_eq!(writes.len(), 2, "{writes:?}");
        assert_eq!(writes[1], REFERENCE_SUBMIT_ENTER);
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 2);
    }

    async fn send(state: &Arc<RemoteGatewayState>, session_id: &str, request_id: &str) -> SendOutcome {
        send_confirmed(state, session_id, "do it".into(), request_id.into(), DEADLINE).await
    }

    fn not_observed() -> SendOutcome {
        SendOutcome::Uncertain {
            code: TURN_NOT_OBSERVED.into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_working_reading_after_the_send_is_started() {
        let session_id = "delivery-started";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);

        assert_eq!(send(&state, session_id, "hub-goal-1").await, SendOutcome::Started);
        assert_one_submit(&backend);
    }

    #[tokio::test(start_paused = true)]
    async fn a_blocked_reading_after_the_send_is_started() {
        let session_id = "delivery-blocked";
        let (_backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Blocked)]);

        assert_eq!(send(&state, session_id, "hub-goal-2").await, SendOutcome::Started);
    }

    #[tokio::test(start_paused = true)]
    async fn a_working_reading_followed_by_idle_before_the_wait_is_still_started() {
        let session_id = "delivery-fast-turn";
        let (_backend, state) = pane(
            session_id,
            vec![
                draft(session_id, AgentControlState::Working),
                draft(session_id, AgentControlState::Idle),
            ],
        );

        assert_eq!(send(&state, session_id, "hub-goal-3").await, SendOutcome::Started);
    }

    #[tokio::test(start_paused = true)]
    async fn a_working_reading_published_by_another_task_after_the_send_is_started() {
        let session_id = "delivery-live";
        let (_backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Idle)]);
        // Subscribed before the send: the Idle the Enter publishes wakes this task, which then
        // publishes Working. Whether send_confirmed sees it through `ready()` or the live
        // subscription depends on scheduling; the test asserts only the outcome.
        let mut receiver = state.agent_events.subscribe();
        let bus = Arc::clone(&state.agent_events);
        let observer = tokio::spawn(async move {
            let idle = receiver.recv().await.expect("the Idle reading");
            assert_eq!(idle.state, AgentControlState::Idle);
            bus.publish(draft("delivery-live", AgentControlState::Working));
        });

        assert_eq!(send(&state, session_id, "hub-goal-4").await, SendOutcome::Started);
        observer.await.expect("observer task");
    }

    #[tokio::test(start_paused = true)]
    async fn a_pane_already_working_before_the_send_with_no_new_reading_is_uncertain() {
        let session_id = "delivery-busy-silent";
        let (_backend, state) = pane(session_id, Vec::new());
        state
            .agent_events
            .publish(draft(session_id, AgentControlState::Working));

        assert_eq!(send(&state, session_id, "hub-goal-11").await, not_observed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_pane_already_working_needs_a_new_screen_revision_after_the_send() {
        let session_id = "delivery-busy-same-screen";
        // The send republishes Working on the same screen: a newer sequence, but no evidence.
        let (_backend, state) =
            pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        state
            .agent_events
            .publish(draft(session_id, AgentControlState::Working));

        assert_eq!(send(&state, session_id, "hub-goal-12").await, not_observed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_pane_already_working_with_a_moved_screen_after_the_send_is_started() {
        let session_id = "delivery-busy-moved";
        let mut moved = draft(session_id, AgentControlState::Working);
        moved.screen_revision = Some("rev-after-send".into());
        let (_backend, state) = pane(session_id, vec![moved]);
        state
            .agent_events
            .publish(draft(session_id, AgentControlState::Working));

        assert_eq!(send(&state, session_id, "hub-goal-13").await, SendOutcome::Started);
    }

    #[tokio::test(start_paused = true)]
    async fn no_reading_is_uncertain_at_the_deadline_after_exactly_one_submit() {
        let session_id = "delivery-silent";
        let (backend, state) = pane(session_id, Vec::new());
        let begun = tokio::time::Instant::now();

        assert_eq!(send(&state, session_id, "hub-goal-5").await, not_observed());
        assert!(begun.elapsed() >= DEADLINE);
        assert_one_submit(&backend);
    }

    #[tokio::test(start_paused = true)]
    async fn a_reading_for_another_session_is_ignored() {
        let session_id = "delivery-own";
        let (_backend, state) = pane(
            session_id,
            vec![draft("delivery-other", AgentControlState::Working)],
        );

        assert_eq!(send(&state, session_id, "hub-goal-6").await, not_observed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_reading_at_or_before_the_cursor_is_ignored() {
        let session_id = "delivery-stale";
        let (_backend, state) = pane(session_id, Vec::new());
        let before = state
            .agent_events
            .publish(draft(session_id, AgentControlState::Working));
        // The cursor is the domain head, which other machine events may have moved further.
        assert!(cursor_now(&state.agent_events) >= before.sequence);

        assert_eq!(send(&state, session_id, "hub-goal-7").await, not_observed());
    }

    #[tokio::test(start_paused = true)]
    async fn an_idle_reading_alone_is_not_a_turn_start() {
        let session_id = "delivery-idle";
        let (_backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Idle)]);

        assert_eq!(send(&state, session_id, "hub-goal-8").await, not_observed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_person_who_just_typed_refuses_with_control_conflict() {
        let session_id = "delivery-human-typing";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        // Same process-global human-input clock exposure as the existing service test.
        crate::remote::server::agent_control_service::note_human_keystrokes(session_id);

        assert_eq!(
            send(&state, session_id, "hub-goal-9").await,
            SendOutcome::Refused {
                code: "CONTROL_CONFLICT".into()
            }
        );
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn an_outcome_unknown_send_is_uncertain_and_written_once() {
        let session_id = "delivery-outcome-unknown";
        let (backend, state) = pane(session_id, Vec::new());
        backend.fail_writes.store(true, Ordering::SeqCst);

        assert_eq!(
            send(&state, session_id, "hub-goal-10").await,
            SendOutcome::Uncertain {
                code: "OPERATION_OUTCOME_UNKNOWN".into()
            }
        );
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn scope_codes_map_to_their_wire_strings() {
        assert_eq!(wire_code(ScopeErrorCode::ControlConflict), "CONTROL_CONFLICT");
        assert_eq!(
            wire_code(ScopeErrorCode::OperationOutcomeUnknown),
            "OPERATION_OUTCOME_UNKNOWN"
        );
    }

    #[test]
    fn the_turn_start_deadline_defaults_and_clamps() {
        assert_eq!(parse_turn_start_ms(None), Duration::from_millis(20_000));
        assert_eq!(parse_turn_start_ms(Some("not a number")), Duration::from_millis(20_000));
        assert_eq!(parse_turn_start_ms(Some("")), Duration::from_millis(20_000));
        assert_eq!(parse_turn_start_ms(Some("5000")), Duration::from_millis(5_000));
        assert_eq!(parse_turn_start_ms(Some(" 7000 ")), Duration::from_millis(7_000));
        assert_eq!(parse_turn_start_ms(Some("10")), Duration::from_millis(2_000));
        assert_eq!(parse_turn_start_ms(Some("999999")), Duration::from_millis(120_000));
    }

    async fn deliver_one(
        state: &Arc<RemoteGatewayState>,
        session_id: &str,
        request_id: &str,
    ) -> Result<DeliveryReport, ScopeError> {
        deliver(state, session_id, "do it".into(), request_id.into(), DEADLINE, None).await
    }

    /// The observer saw the pane idle with no prompt: ready for a delivery.
    fn ready_pane(state: &Arc<RemoteGatewayState>, session_id: &str) {
        state
            .agent_events
            .publish(draft(session_id, AgentControlState::Idle));
    }

    fn run_id(state: &Arc<RemoteGatewayState>, session_id: &str, sequence: u64) -> String {
        ferryx_hub::agent_control::pty_run_id(session_id, state.agent_events.instance(), sequence)
    }

    #[tokio::test(start_paused = true)]
    async fn a_start_caused_only_by_this_delivery_is_its_run() {
        let session_id = "deliver-causal";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        ready_pane(&state, session_id);
        let cursor = cursor_now(&state.agent_events);

        let report = deliver_one(&state, session_id, "sv-goal-r1-d1").await.expect("typed");
        let start = state.agent_events.latest(session_id).expect("the start reading");
        assert!(start.sequence > cursor);
        assert_eq!(
            report,
            DeliveryReport::Started {
                request_id: "sv-goal-r1-d1".into(),
                session_id: session_id.into(),
                run_id: run_id(&state, session_id, start.sequence),
            }
        );
        assert_eq!(
            ferryx_hub::agent_control::pty_run_start(&run_id(&state, session_id, start.sequence), session_id),
            Some((state.agent_events.instance(), start.sequence))
        );
        assert_one_submit(&backend);
    }

    #[tokio::test(start_paused = true)]
    async fn a_start_after_other_input_reached_the_pane_is_not_this_deliverys() {
        let session_id = "deliver-foreign";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Idle)]);
        ready_pane(&state, session_id);
        // Subscribed before the send: when the pane reacts to the Enter, a phone chat message
        // (another request id) is typed into the same pane, then the pane starts working.
        let mut receiver = state.agent_events.subscribe();
        let bus = Arc::clone(&state.agent_events);
        let other = tokio::spawn(async move {
            let idle = receiver.recv().await.expect("the Idle reading");
            assert_eq!(idle.session_id, "deliver-foreign");
            bus.note_input("deliver-foreign", Some("phone-chat-1"));
            bus.publish(draft("deliver-foreign", AgentControlState::Working));
        });

        let report = deliver_one(&state, session_id, "sv-goal-r1-d1").await.expect("typed");
        other.await.expect("the other input");
        assert_eq!(
            report,
            DeliveryReport::Accepted {
                request_id: "sv-goal-r1-d1".into(),
                session_id: session_id.into(),
                code: FOREIGN_INPUT.into(),
            }
        );
        assert_one_submit(&backend);
    }

    #[tokio::test(start_paused = true)]
    async fn activity_of_another_pane_is_never_this_deliverys_start() {
        let session_id = "deliver-own";
        let (backend, state) = pane(
            session_id,
            vec![draft("deliver-other", AgentControlState::Working)],
        );
        ready_pane(&state, session_id);

        assert_eq!(
            deliver_one(&state, session_id, "sv-goal-r1-d2").await.expect("typed"),
            DeliveryReport::Accepted {
                request_id: "sv-goal-r1-d2".into(),
                session_id: session_id.into(),
                code: TURN_NOT_OBSERVED.into(),
            }
        );
        assert_one_submit(&backend);
    }

    #[tokio::test(start_paused = true)]
    async fn a_refused_delivery_types_nothing() {
        let session_id = "deliver-human-typing";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        ready_pane(&state, session_id);
        crate::remote::server::agent_control_service::note_human_keystrokes(session_id);

        let error = deliver_one(&state, session_id, "sv-goal-r1-d3").await.expect_err("refused");
        assert_eq!(error.code, ScopeErrorCode::ControlConflict);
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 0);
    }

    fn not_ready_report(session_id: &str, request_id: &str) -> DeliveryReport {
        DeliveryReport::NotReady {
            request_id: request_id.into(),
            session_id: session_id.into(),
            code: ferryx_hub::agent_control::PANE_NOT_READY.into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_busy_pane_is_never_typed_into() {
        let session_id = "deliver-busy";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        // The agent is mid-turn: its next reading belongs to that turn, not to this request.
        let mut busy = draft(session_id, AgentControlState::Working);
        busy.screen_revision = Some("rev-earlier-turn".into());
        state.agent_events.publish(busy);
        let begun = tokio::time::Instant::now();

        assert_eq!(
            deliver_one(&state, session_id, "sv-goal-r1-d4").await.expect("not an error"),
            not_ready_report(session_id, "sv-goal-r1-d4")
        );
        assert!(begun.elapsed() >= PANE_READY_WAIT, "it waited the bounded time for readiness");
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 0, "nothing was typed");
    }

    #[tokio::test(start_paused = true)]
    async fn a_pane_with_no_readings_is_never_typed_into() {
        let session_id = "deliver-fresh";
        // A pane launched just now: no reading yet, so a boot-time reading could pass for a start.
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);

        assert_eq!(
            deliver_one(&state, session_id, "sv-goal-r1-d5").await.expect("not an error"),
            not_ready_report(session_id, "sv-goal-r1-d5")
        );
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 0, "nothing was typed");
    }

    #[tokio::test(start_paused = true)]
    async fn a_busy_pane_that_turns_idle_within_the_wait_is_delivered_to() {
        let session_id = "deliver-busy-then-idle";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        state
            .agent_events
            .publish(draft(session_id, AgentControlState::Working));
        let mut delivery = Box::pin(deliver_one(&state, session_id, "sv-goal-r1-d6"));
        // One poll: the delivery is now subscribed and waiting for the pane to be ready.
        tokio::select! {
            biased;
            _ = &mut delivery => panic!("a busy pane is not delivered to at once"),
            () = std::future::ready(()) => {}
        }
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 0);
        ready_pane(&state, session_id);

        let report = delivery.await.expect("typed");
        let start = state.agent_events.latest(session_id).expect("the start reading");
        assert_eq!(start.state, AgentControlState::Working);
        assert_eq!(
            report,
            DeliveryReport::Started {
                request_id: "sv-goal-r1-d6".into(),
                session_id: session_id.into(),
                run_id: run_id(&state, session_id, start.sequence),
            }
        );
        assert_one_submit(&backend);
    }

    #[tokio::test(start_paused = true)]
    async fn input_of_another_request_with_no_reading_yet_keeps_the_pane_unready() {
        let session_id = "deliver-stale-ready";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        ready_pane(&state, session_id);
        // A phone chat message is typed in full; the observer has not published its turn yet
        // (its Enter publishes nothing), so the latest reading is still the stale Idle.
        backend.skip_enters.store(1, Ordering::SeqCst);
        agent_control_service::send_text(
            &state,
            session_id.into(),
            "a message from the phone".into(),
            Some("phone-chat-1".into()),
            None,
            None,
        )
        .await
        .expect("the other message was typed");
        let begun = tokio::time::Instant::now();

        assert_eq!(
            deliver_one(&state, session_id, "sv-goal-r1-d9").await.expect("not an error"),
            not_ready_report(session_id, "sv-goal-r1-d9"),
            "the stale Idle cannot carry this request's start"
        );
        assert!(begun.elapsed() >= PANE_READY_WAIT, "it waited the bounded time for readiness");
        assert_eq!(
            backend.writes.lock().expect("writes").len(),
            2,
            "only the other message was typed"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_pane_idle_after_other_input_is_delivered_to_once_the_observer_settles() {
        let session_id = "deliver-settled-ready";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        ready_pane(&state, session_id);
        backend.skip_enters.store(1, Ordering::SeqCst);
        agent_control_service::send_text(
            &state,
            session_id.into(),
            "a message from the phone".into(),
            Some("phone-chat-2".into()),
            None,
            None,
        )
        .await
        .expect("the other message was typed");
        let settles = state
            .agent_events
            .input_settles_at(session_id, input_settle_horizon())
            .expect("the other message was marked");
        // The observer reads the pane idle after that message, but within its settle horizon.
        let mut after = draft(session_id, AgentControlState::Idle);
        after.screen_revision = Some("rev-after-the-phone-message".into());
        state.agent_events.publish(after);

        let report = deliver_one(&state, session_id, "sv-goal-r1-d10").await.expect("typed");
        assert!(
            tokio::time::Instant::now() >= settles,
            "nothing is typed before the other message is past the observer's settle horizon"
        );
        let start = state.agent_events.latest(session_id).expect("the start reading");
        assert_eq!(
            report,
            DeliveryReport::Started {
                request_id: "sv-goal-r1-d10".into(),
                session_id: session_id.into(),
                run_id: run_id(&state, session_id, start.sequence),
            }
        );
        assert_eq!(backend.writes.lock().expect("writes").len(), 4);
    }

    #[tokio::test(start_paused = true)]
    async fn an_input_queued_ahead_and_typed_after_the_readiness_check_keeps_the_delivery_untyped() {
        let session_id = "deliver-queued-ahead";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        ready_pane(&state, session_id);
        // The input ahead is a phone chat message: another request id typed through the same
        // per-pane queue and writer. It holds the pane's step inside its first write until
        // released, and its Enter publishes nothing; the delivery's Enter publishes Working.
        backend.skip_enters.store(1, Ordering::SeqCst);
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        *backend.gate.lock().expect("gate") = Some((entered_tx, release_rx));
        let before = state.agent_events.input_cursor();
        let ahead_state = Arc::clone(&state);
        let ahead = tokio::spawn(async move {
            agent_control_service::send_text(
                &ahead_state,
                "deliver-queued-ahead".into(),
                "a message from the phone".into(),
                Some("phone-chat-1".into()),
                None,
                None,
            )
            .await
        });
        entered_rx.await.expect("the input ahead is being typed");
        // Marked at the write itself, while its transaction is still running: a mark issued
        // only after the transaction joins would leave the ledger unchanged here.
        assert_eq!(state.agent_events.input_cursor(), before + 1, "the first write was marked");
        // The observer then reads the pane idle again and the first write settles, so the
        // readiness check before the cursor passes; the Enter ahead is still to come.
        let mut after = draft(session_id, AgentControlState::Idle);
        after.screen_revision = Some("rev-composer-filled".into());
        state.agent_events.publish(after);
        tokio::time::advance(input_settle_horizon()).await;

        // One poll: the delivery passes its readiness check, takes its cursors now and queues
        // behind the input ahead.
        let mut delivery = Box::pin(deliver_one(&state, session_id, "sv-goal-r1-d7"));
        tokio::select! {
            biased;
            _ = &mut delivery => panic!("the delivery cannot finish while the step is held"),
            () = std::future::ready(()) => {}
        }
        let cursor = state.agent_events.input_cursor();
        release_tx.send(()).expect("release the input ahead");
        ahead.await.expect("the input ahead").expect("typed");
        assert_eq!(state.agent_events.input_cursor(), cursor + 1, "the Enter ahead was marked");

        assert_eq!(
            delivery.await.expect("typed"),
            not_ready_report(session_id, "sv-goal-r1-d7"),
            "the Enter ahead reached the pane after the readiness check: under the step the \
             pane is no longer observed ready, so nothing is typed"
        );
        let writes = backend.writes.lock().expect("writes").clone();
        assert_eq!(writes.len(), 2, "only the message ahead was typed: {writes:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_keystroke_in_the_pane_then_typing_in_another_pane_is_still_foreign() {
        let session_id = "deliver-keystroke";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Idle)]);
        ready_pane(&state, session_id);
        // Subscribed before the send: when the pane reacts to the Enter, a person types a key
        // into this pane, then into another pane, then this pane starts working.
        let mut receiver = state.agent_events.subscribe();
        let bus = Arc::clone(&state.agent_events);
        let person = tokio::spawn(async move {
            let idle = receiver.recv().await.expect("the Idle reading");
            assert_eq!(idle.session_id, "deliver-keystroke");
            agent_control_service::note_raw_write(&bus, "deliver-keystroke");
            for _ in 0..64 {
                agent_control_service::note_raw_write(&bus, "deliver-keystroke-other");
            }
            bus.publish(draft("deliver-keystroke", AgentControlState::Working));
        });

        let report = deliver_one(&state, session_id, "sv-goal-r1-d8").await.expect("typed");
        person.await.expect("the person typing");
        assert_eq!(
            report,
            DeliveryReport::Accepted {
                request_id: "sv-goal-r1-d8".into(),
                session_id: session_id.into(),
                code: FOREIGN_INPUT.into(),
            },
            "typing elsewhere never erases this pane's keystroke"
        );
        assert_one_submit(&backend);
    }

    fn scope(code: ScopeErrorCode) -> ScopeError {
        ScopeError {
            code,
            message: "x".into(),
            retryable: false,
            details: serde_json::Value::Null,
        }
    }

    #[test]
    fn delivery_reports_map_to_hub_stages() {
        use ferryx_hub::agent_control::{Delivery, StartReceipt};
        let started = DeliveryReport::Started {
            request_id: "sv-1".into(),
            session_id: "pane".into(),
            run_id: "pty:pane:e1:4".into(),
        };
        let wire = serde_json::to_value(&started).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"stage": "started", "requestId": "sv-1", "sessionId": "pane", "runId": "pty:pane:e1:4"})
        );
        assert_eq!(
            hub_delivery_from_reply(&serde_json::json!({ "delivery": wire })),
            Delivery::Started(StartReceipt {
                request_id: "sv-1".into(),
                session_id: "pane".into(),
                run_id: "pty:pane:e1:4".into(),
            })
        );
        // A typed send without a readable receipt is never a start.
        for reply in [serde_json::json!({}), serde_json::json!({"delivery": {"stage": "started"}})] {
            assert_eq!(
                hub_delivery_from_reply(&reply),
                Delivery::Uncertain {
                    accepted: true,
                    code: START_RECEIPT_MISSING.into()
                }
            );
        }
        assert_eq!(
            hub_delivery(Ok(DeliveryReport::Accepted {
                request_id: "sv-1".into(),
                session_id: "pane".into(),
                code: FOREIGN_INPUT.into(),
            })),
            Delivery::Uncertain {
                accepted: true,
                code: FOREIGN_INPUT.into()
            }
        );
        assert_eq!(
            hub_delivery(Err(scope(ScopeErrorCode::ControlConflict))),
            Delivery::Refused {
                code: "CONTROL_CONFLICT".into()
            }
        );
        assert_eq!(
            hub_delivery(Err(scope(ScopeErrorCode::OperationOutcomeUnknown))),
            Delivery::Uncertain {
                accepted: false,
                code: "OPERATION_OUTCOME_UNKNOWN".into()
            }
        );
        // A pane that was not ready typed nothing: released and held.
        let not_ready = serde_json::to_value(not_ready_report("pane", "sv-1")).unwrap();
        assert_eq!(
            not_ready,
            serde_json::json!({"stage": "notReady", "requestId": "sv-1", "sessionId": "pane", "code": "PANE_NOT_READY"})
        );
        assert_eq!(
            hub_delivery_from_reply(&serde_json::json!({ "delivery": not_ready })),
            Delivery::Refused {
                code: ferryx_hub::agent_control::PANE_NOT_READY.into()
            }
        );
        // A failure that can follow a write is never a refusal.
        assert_eq!(
            hub_delivery(Err(scope(ScopeErrorCode::Timeout))),
            Delivery::Uncertain {
                accepted: false,
                code: "TIMEOUT".into()
            }
        );
    }

    #[test]
    fn a_delivery_waits_the_asked_time_within_bounds() {
        assert_eq!(observe_deadline(0), turn_start_deadline());
        assert_eq!(observe_deadline(5_000), Duration::from_millis(5_000));
        assert_eq!(observe_deadline(1), Duration::from_millis(TURN_START_MIN_MS));
        assert_eq!(observe_deadline(u64::MAX), Duration::from_millis(TURN_START_MAX_MS));
    }
}
