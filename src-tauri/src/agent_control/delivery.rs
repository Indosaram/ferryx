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

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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
/// The observer cursor and the input cursor are both taken BEFORE the send (subscribe before
/// trigger). A start is a reading of this session newer than that cursor that is Working,
/// Blocked or carries a prompt (on a pane already busy before the send, with a moved screen,
/// see [`PreSend`]). It is attributed only when no other input reached this pane since the
/// cursor: no other request's mark ([`AgentEventBus::foreign_input_since`]) and no raw
/// keystrokes. Activity of another session, or of this pane caused by someone else, never
/// starts this request's run. Never retries the send.
pub async fn deliver(
    state: &Arc<RemoteGatewayState>,
    session_id: &str,
    text: String,
    request_id: String,
    deadline: Duration,
    authorize: Option<Arc<dyn Fn() -> Result<(), ScopeError> + Send + Sync>>,
) -> Result<DeliveryReport, ScopeError> {
    let bus = Arc::clone(&state.agent_events);
    let origin = cursor_now(&bus);
    let inputs = bus.input_cursor();
    let pre = PreSend::of(&bus, session_id);
    let sent_at_ms = agent_control_service::input_clock_ms();
    agent_control_service::send_text(
        state,
        session_id.to_string(),
        text,
        Some(request_id.clone()),
        authorize,
    )
    .await?;

    let attribute = |observation: &AgentObservation| {
        let foreign = bus.foreign_input_since(session_id, inputs, &request_id)
            || agent_control_service::raw_input_since(session_id, sent_at_ms);
        if foreign {
            DeliveryReport::Accepted {
                request_id: request_id.clone(),
                session_id: session_id.to_string(),
                code: FOREIGN_INPUT.to_string(),
            }
        } else {
            DeliveryReport::Started {
                request_id: request_id.clone(),
                session_id: session_id.to_string(),
                run_id: ferryx_hub::agent_control::pty_run_id(session_id, observation.sequence),
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
/// or the send's own refusal (nothing typed) or unknown outcome.
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
        Err(error) => {
            let code = wire_code(error.code);
            match refusal_outcome(&error) {
                SendOutcome::Uncertain { .. } => Delivery::Uncertain {
                    accepted: false,
                    code,
                },
                SendOutcome::Started | SendOutcome::Refused { .. } => Delivery::Refused { code },
            }
        }
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

    const DEADLINE: Duration = Duration::from_secs(20);

    /// One shell pane. Writing a submit's Enter publishes `on_enter` to the observer bus, which
    /// stands in for the observer seeing the pane react to the send. Every write is recorded.
    struct Pane {
        session_id: String,
        writes: Mutex<Vec<Vec<u8>>>,
        write_attempts: AtomicUsize,
        fail_writes: AtomicBool,
        bus: OnceLock<Arc<AgentEventBus>>,
        on_enter: Mutex<Vec<ObservationDraft>>,
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
            if data == REFERENCE_SUBMIT_ENTER {
                let bus = self.bus.get().expect("bus attached");
                for draft in self.on_enter.lock().expect("on_enter").drain(..) {
                    bus.publish(draft);
                }
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

    #[tokio::test(start_paused = true)]
    async fn a_start_caused_only_by_this_delivery_is_its_run() {
        let session_id = "deliver-causal";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Working)]);
        let cursor = cursor_now(&state.agent_events);

        let report = deliver_one(&state, session_id, "sv-goal-r1-d1").await.expect("typed");
        let start = state.agent_events.latest(session_id).expect("the start reading");
        assert!(start.sequence > cursor);
        assert_eq!(
            report,
            DeliveryReport::Started {
                request_id: "sv-goal-r1-d1".into(),
                session_id: session_id.into(),
                run_id: format!("pty:{session_id}:{}", start.sequence),
            }
        );
        assert_eq!(
            ferryx_hub::agent_control::pty_run_sequence(&format!("pty:{session_id}:{}", start.sequence), session_id),
            Some(start.sequence)
        );
        assert_one_submit(&backend);
    }

    #[tokio::test(start_paused = true)]
    async fn a_start_after_other_input_reached_the_pane_is_not_this_deliverys() {
        let session_id = "deliver-foreign";
        let (backend, state) = pane(session_id, vec![draft(session_id, AgentControlState::Idle)]);
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
        crate::remote::server::agent_control_service::note_human_keystrokes(session_id);

        let error = deliver_one(&state, session_id, "sv-goal-r1-d3").await.expect_err("refused");
        assert_eq!(error.code, ScopeErrorCode::ControlConflict);
        assert_eq!(backend.write_attempts.load(Ordering::SeqCst), 0);
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
            run_id: "pty:pane:4".into(),
        };
        let wire = serde_json::to_value(&started).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"stage": "started", "requestId": "sv-1", "sessionId": "pane", "runId": "pty:pane:4"})
        );
        assert_eq!(
            hub_delivery_from_reply(&serde_json::json!({ "delivery": wire })),
            Delivery::Started(StartReceipt {
                request_id: "sv-1".into(),
                session_id: "pane".into(),
                run_id: "pty:pane:4".into(),
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
    }

    #[test]
    fn a_delivery_waits_the_asked_time_within_bounds() {
        assert_eq!(observe_deadline(0), turn_start_deadline());
        assert_eq!(observe_deadline(5_000), Duration::from_millis(5_000));
        assert_eq!(observe_deadline(1), Duration::from_millis(TURN_START_MIN_MS));
        assert_eq!(observe_deadline(u64::MAX), Duration::from_millis(TURN_START_MAX_MS));
    }
}
