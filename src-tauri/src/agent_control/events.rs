//! Agent-control observations and the replay log a supervisor waits on.
//!
//! The sequence domain is the machine-events sequence when that stream is attached, so a change
//! the events socket already carries is the same number `agents events` and `agents wait` use.
//! The log keeps a bounded tail. A cursor that falls out of the tail is a gap plus a snapshot of
//! the latest observation per session, the same recovery `inventoryInvalidated` asks for. The
//! prompt body never enters the log: it is fetched with `agents read`.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use super::{derive_status, AgentControlState, AgentStateSource, AgentTier, ProviderReport};

/// Machine-events type for one slim agent-control change.
pub const AGENT_CONTROL_EVENT: &str = "agentControlChanged";

/// How many observations the log retains. Older cursors come back as a gap.
pub const AGENT_EVENT_RETAIN: usize = 256;

/// How many input marks the bus keeps per session. A cursor older than the oldest kept mark
/// counts as if another input had arrived.
const INPUT_MARKS_PER_SESSION: usize = 32;

/// One change a supervisor can wait for. The prompt body is intentionally absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentObservation {
    pub sequence: u64,
    pub session_id: String,
    pub state: AgentControlState,
    pub state_source: AgentStateSource,
    pub tier: AgentTier,
    pub prompt_id: Option<String>,
    pub kind: Option<String>,
    pub screen_revision: Option<String>,
}

impl AgentObservation {
    /// The slim object published inside a machine event. The events API adds `sequence`
    /// beside it so a supervisor can advance its cursor; the prompt body stays absent.
    pub fn slim(&self) -> Value {
        json!({
            "sessionId": self.session_id,
            "state": self.state,
            "stateSource": self.state_source,
            "tier": self.tier,
            "promptId": self.prompt_id,
            "kind": self.kind,
            "screenRevision": self.screen_revision,
        })
    }

    /// Slim payload plus the sequence a waiter advances.
    pub fn sequenced(&self) -> Value {
        let mut value = self.slim();
        value["sequence"] = json!(self.sequence);
        value
    }

    pub fn same_reading(&self, other: &ObservationDraft) -> bool {
        self.session_id == other.session_id
            && self.state == other.state
            && self.state_source == other.state_source
            && self.tier == other.tier
            && self.prompt_id == other.prompt_id
            && self.kind == other.kind
            && self.screen_revision == other.screen_revision
    }
}

/// A reading that does not yet have a sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct ObservationDraft {
    pub session_id: String,
    pub state: AgentControlState,
    pub state_source: AgentStateSource,
    pub tier: AgentTier,
    pub prompt_id: Option<String>,
    pub kind: Option<String>,
    pub screen_revision: Option<String>,
    pub workspace_id: Option<String>,
}

/// What `events` returns. A gap carries the current snapshot instead of a partial tail.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentReplay {
    Events {
        events: Vec<AgentObservation>,
        after_sequence: u64,
    },
    Gap {
        snapshot: Vec<AgentObservation>,
        after_sequence: u64,
    },
}

struct Log {
    revision: u64,
    /// Highest sequence that fell out of `retained`. A cursor below it missed an observation.
    /// Sequences are shared with other machine events, so a hole in `retained` is not a gap.
    evicted_through: u64,
    retained: VecDeque<AgentObservation>,
    latest: HashMap<String, AgentObservation>,
    capacity: usize,
}

/// Which inputs reached each pane, in arrival order. A delivery reads it to tell whether a turn
/// start can only have been caused by its own input.
#[derive(Default)]
struct InputLedger {
    /// Last mark issued; marks count up across every session.
    issued: u64,
    sessions: HashMap<String, SessionInputs>,
}

#[derive(Default)]
struct SessionInputs {
    /// (mark, request id of the input; `None` when it had none).
    marks: VecDeque<(u64, Option<String>)>,
    /// Highest mark that fell out of `marks`.
    evicted_through: u64,
}

/// Retained agent-control observations. Publishing also forwards a slim machine event when the
/// machine stream is attached.
pub struct AgentEventBus {
    log: Mutex<Log>,
    tx: broadcast::Sender<AgentObservation>,
    machine_events: Mutex<Option<Arc<crate::remote::machine_events::MachineEvents>>>,
    local_sequence: Mutex<u64>,
    inputs: Mutex<InputLedger>,
    /// This bus's observer epoch. Sequences restart with the daemon, so a sequence is only
    /// comparable with others of the same epoch; run ids and wait replies carry it.
    instance: String,
}

impl AgentEventBus {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        let (tx, _) = broadcast::channel(capacity);
        Self {
            log: Mutex::new(Log {
                revision: 0,
                evicted_through: 0,
                retained: VecDeque::new(),
                latest: HashMap::new(),
                capacity,
            }),
            tx,
            machine_events: Mutex::new(None),
            local_sequence: Mutex::new(0),
            inputs: Mutex::new(InputLedger::default()),
            instance: uuid::Uuid::new_v4().simple().to_string(),
        }
    }

    /// The observer epoch of every sequence this bus issues.
    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// The latest reading of every session, by session id.
    pub fn snapshot(&self) -> Vec<AgentObservation> {
        snapshot_of(&self.log.lock().latest)
    }

    pub fn attach_machine_events(&self, events: Arc<crate::remote::machine_events::MachineEvents>) {
        *self.machine_events.lock() = Some(events);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AgentObservation> {
        self.tx.subscribe()
    }

    pub fn latest(&self, session_id: &str) -> Option<AgentObservation> {
        self.log.lock().latest.get(session_id).cloned()
    }

    pub fn revision(&self) -> u64 {
        self.log.lock().revision
    }

    /// The newest input mark. Take it before a send; [`Self::foreign_input_since`] compares
    /// against it.
    pub fn input_cursor(&self) -> u64 {
        self.inputs.lock().issued
    }

    /// Record that input under `request_id` (`None`: raw keys) is reaching `session_id`. Every
    /// writer calls it right before its bytes go out: the per-target input queue's writer for
    /// chat, agent-control and phone input, and each raw write path for keystrokes and pastes.
    pub fn note_input(&self, session_id: &str, request_id: Option<&str>) {
        let mut guard = self.inputs.lock();
        let ledger = &mut *guard;
        ledger.issued = ledger.issued.saturating_add(1);
        let mark = ledger.issued;
        let session = ledger.sessions.entry(session_id.to_string()).or_default();
        session.marks.push_back((mark, request_id.map(str::to_string)));
        while session.marks.len() > INPUT_MARKS_PER_SESSION {
            if let Some((evicted, _)) = session.marks.pop_front() {
                session.evicted_through = session.evicted_through.max(evicted);
            }
        }
    }

    /// Whether input other than `request_id`'s reached `session_id` after the mark `after`.
    /// Marks that fell out of the bounded ledger after `after` count as foreign.
    pub fn foreign_input_since(&self, session_id: &str, after: u64, request_id: &str) -> bool {
        let ledger = self.inputs.lock();
        let Some(session) = ledger.sessions.get(session_id) else {
            return false;
        };
        session.evicted_through > after
            || session
                .marks
                .iter()
                .any(|(mark, id)| *mark > after && id.as_deref() != Some(request_id))
    }

    /// Newest sequence issued in the shared domain or logged, whichever is higher.
    ///
    /// An observation holds its sequence (`allocate_sequence`) before `publish` logs it, so the
    /// log revision alone can sit below a reading already in flight. A cursor taken from `head`
    /// is at or past every such reading. `replay(head())` stays an `Events` cursor: the domain
    /// head and the revision only grow, so `after <= domain_head().max(revision)` still holds
    /// when `replay` checks it (only an eviction past the cursor makes it a gap).
    pub fn head(&self) -> u64 {
        let domain = self.domain_head();
        domain.max(self.log.lock().revision)
    }

    /// Record one changed reading and wake waiters. Unchanged readings are the caller's to drop.
    pub fn publish(&self, draft: ObservationDraft) -> AgentObservation {
        let sequence = self.allocate_sequence(&draft);
        self.record(sequence, draft)
    }

    /// Log a reading under a sequence `allocate_sequence` already issued and wake waiters.
    fn record(&self, sequence: u64, draft: ObservationDraft) -> AgentObservation {
        let observation = AgentObservation {
            sequence,
            session_id: draft.session_id,
            state: draft.state,
            state_source: draft.state_source,
            tier: draft.tier,
            prompt_id: draft.prompt_id,
            kind: draft.kind,
            screen_revision: draft.screen_revision,
        };
        {
            let mut log = self.log.lock();
            log.revision = log.revision.max(sequence);
            log.latest
                .insert(observation.session_id.clone(), observation.clone());
            log.retained.push_back(observation.clone());
            while log.retained.len() > log.capacity {
                if let Some(evicted) = log.retained.pop_front() {
                    log.evicted_through = log.evicted_through.max(evicted.sequence);
                }
            }
        }
        let _ = self.tx.send(observation.clone());
        observation
    }

    fn allocate_sequence(&self, draft: &ObservationDraft) -> u64 {
        if let Some(events) = self.machine_events.lock().clone() {
            return events.publish_sequenced(
                AGENT_CONTROL_EVENT,
                draft.workspace_id.as_deref(),
                Some(draft.session_id.as_str()),
                AgentObservation {
                    sequence: 0,
                    session_id: draft.session_id.clone(),
                    state: draft.state,
                    state_source: draft.state_source,
                    tier: draft.tier,
                    prompt_id: draft.prompt_id.clone(),
                    kind: draft.kind.clone(),
                    screen_revision: draft.screen_revision.clone(),
                }
                .slim(),
            );
        }
        let mut sequence = self.local_sequence.lock();
        *sequence = sequence.saturating_add(1);
        *sequence
    }

    /// Newest sequence issued in the shared domain, agent observation or not.
    fn domain_head(&self) -> u64 {
        let attached = self.machine_events.lock().clone();
        match attached {
            Some(events) => events.sequence(),
            None => *self.local_sequence.lock(),
        }
    }

    pub fn replay(&self, after: u64) -> AgentReplay {
        let head = self.domain_head();
        let log = self.log.lock();
        let snapshot = snapshot_of(&log.latest);
        // A cursor past every issued sequence belongs to another daemon run. A cursor inside
        // the domain but past the last agent observation is current, not a gap: other machine
        // events share these numbers.
        if after > head.max(log.revision) || after < log.evicted_through {
            return AgentReplay::Gap {
                snapshot,
                after_sequence: log.revision,
            };
        }
        AgentReplay::Events {
            events: log
                .retained
                .iter()
                .filter(|event| event.sequence > after)
                .cloned()
                .collect(),
            // Never the domain head: an observation can hold a sequence before it is logged.
            after_sequence: log.revision.max(after),
        }
    }
}

fn snapshot_of(latest: &HashMap<String, AgentObservation>) -> Vec<AgentObservation> {
    let mut items: Vec<_> = latest.values().cloned().collect();
    items.sort_by(|left, right| left.session_id.cmp(&right.session_id));
    items
}

/// Whether a wait's `until` matches this observation.
pub fn matches_until(observation: &AgentObservation, until: &str) -> bool {
    match until {
        "any" => true,
        "prompt" => observation.prompt_id.is_some(),
        "working" => observation.state == AgentControlState::Working,
        "blocked" => observation.state == AgentControlState::Blocked,
        "idle" => observation.state == AgentControlState::Idle,
        _ => false,
    }
}

pub fn until_is_known(until: &str) -> bool {
    matches!(until, "any" | "prompt" | "working" | "blocked" | "idle")
}

/// A wait either sees the reading it asked for, or learns its cursor fell out of the log.
pub enum WaitHit {
    Observation(AgentObservation),
    Gap {
        snapshot: Vec<AgentObservation>,
        after_sequence: u64,
    },
}

fn revision_advanced(observation: &AgentObservation, after_revision: Option<&str>) -> bool {
    match after_revision {
        Some(revision) => observation.screen_revision.as_deref() != Some(revision),
        None => true,
    }
}

fn acceptable(
    observation: &AgentObservation,
    session_id: &str,
    until: &str,
    after: u64,
    after_revision: Option<&str>,
) -> bool {
    observation.session_id == session_id
        && observation.sequence > after
        && matches_until(observation, until)
        && revision_advanced(observation, after_revision)
}

fn ready(
    bus: &AgentEventBus,
    session_id: &str,
    until: &str,
    after: u64,
    after_revision: Option<&str>,
) -> Option<WaitHit> {
    match bus.replay(after) {
        AgentReplay::Gap {
            snapshot,
            after_sequence,
        } => Some(WaitHit::Gap {
            snapshot,
            after_sequence,
        }),
        AgentReplay::Events { events, .. } => events
            .into_iter()
            .rev()
            .find(|event| acceptable(event, session_id, until, after, after_revision))
            .map(WaitHit::Observation),
    }
}

/// Block until `until` matches a newer reading, the log gaps, or `timeout` elapses.
///
/// `None` is the timeout. A gap is returned immediately so the caller resyncs from the snapshot
/// instead of waiting on a cursor the log no longer has.
pub async fn wait_for(
    bus: &AgentEventBus,
    session_id: &str,
    until: &str,
    after: u64,
    after_revision: Option<&str>,
    timeout: std::time::Duration,
) -> Option<WaitHit> {
    if let Some(hit) = ready(bus, session_id, until, after, after_revision) {
        return Some(hit);
    }
    let mut receiver = bus.subscribe();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        match tokio::time::timeout(remaining, receiver.recv()).await {
            Err(_) => return None,
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                return ready(bus, session_id, until, after, after_revision)
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                if let Some(hit) = ready(bus, session_id, until, after, after_revision) {
                    return Some(hit);
                }
            }
            Ok(Ok(observation)) => {
                if acceptable(&observation, session_id, until, after, after_revision) {
                    return Some(WaitHit::Observation(observation));
                }
                if let AgentReplay::Gap {
                    snapshot,
                    after_sequence,
                } = bus.replay(after)
                {
                    return Some(WaitHit::Gap {
                        snapshot,
                        after_sequence,
                    });
                }
            }
        }
    }
}

/// Screen + provider reading the observer publishes.
///
/// A self-reported provider state is left alone. A prompt on the screen is blocked. For every
/// other screen-only reading, `quiet_elapsed` (the quiet timer, not a poll) moves Working and
/// Unknown to Idle.
pub fn classify_screen(
    provider: Option<&ProviderReport>,
    cols: u16,
    rows: u16,
    text: &str,
    quiet_elapsed: bool,
) -> ObservationDraft {
    let detection = crate::agent_detect::default_engine().detect(
        &crate::agent_detect::ScreenInput {
            rows: text.lines().map(str::to_string).collect(),
            title: String::new(),
        },
        None,
    );
    let agent = provider
        .and_then(|report| report.agent.clone())
        .filter(|id| !id.trim().is_empty())
        .or_else(|| {
            detection
                .as_ref()
                .map(|found| found.manifest_id.clone())
                .filter(|id| !id.is_empty())
        });
    let pending = agent.as_ref().and_then(|agent| {
        crate::remote::reference_chat::prompts::detect_reference_prompt_in_session(
            agent,
            text,
            &crate::remote::reference_chat::prompts::ReferencePromptSession::default(),
        )
    });
    let mut status = derive_status(
        provider,
        detection.as_ref().map(|found| found.state),
        pending.is_some(),
    );
    if quiet_elapsed
        && status.source != AgentStateSource::Provider
        && pending.is_none()
        && !matches!(
            status.state,
            AgentControlState::Blocked | AgentControlState::Idle
        )
    {
        status.state = AgentControlState::Idle;
        status.source = AgentStateSource::Screen;
    }
    let revision =
        crate::remote::reference_chat::screen::reference_screen_revision(cols, rows, text);
    ObservationDraft {
        session_id: String::new(),
        state: status.state,
        state_source: status.source,
        tier: status.tier,
        prompt_id: pending.as_ref().map(|prompt| prompt.id.clone()),
        kind: pending.map(|prompt| prompt_kind(&prompt.kind)),
        screen_revision: Some(revision),
        workspace_id: None,
    }
}

fn prompt_kind(kind: &crate::remote::reference_chat::types::ReferencePromptKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "question".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(session: &str, state: AgentControlState) -> ObservationDraft {
        ObservationDraft {
            session_id: session.into(),
            state,
            state_source: AgentStateSource::Screen,
            tier: AgentTier::Screen,
            prompt_id: None,
            kind: None,
            screen_revision: Some("rev".into()),
            workspace_id: None,
        }
    }

    #[test]
    fn replay_returns_the_tail_and_a_gap_when_the_cursor_fell_out() {
        let bus = AgentEventBus::new(2);
        let first = bus.publish(draft("s", AgentControlState::Idle));
        let second = bus.publish(draft("s", AgentControlState::Working));
        let third = bus.publish(draft("s", AgentControlState::Blocked));
        assert!(first.sequence < second.sequence && second.sequence < third.sequence);

        match bus.replay(0) {
            AgentReplay::Gap {
                snapshot,
                after_sequence,
            } => {
                assert_eq!(after_sequence, third.sequence);
                assert_eq!(snapshot.len(), 1);
                assert_eq!(snapshot[0].state, AgentControlState::Blocked);
            }
            AgentReplay::Events { .. } => panic!("the dropped head must be a gap"),
        }

        match bus.replay(first.sequence) {
            AgentReplay::Events {
                events,
                after_sequence,
            } => {
                assert_eq!(after_sequence, third.sequence);
                assert_eq!(
                    events
                        .iter()
                        .map(|event| event.sequence)
                        .collect::<Vec<_>>(),
                    vec![second.sequence, third.sequence]
                );
            }
            AgentReplay::Gap { .. } => panic!("a cursor still inside the tail is not a gap"),
        }
    }

    #[test]
    fn a_fresh_cursor_is_not_a_gap_when_other_machine_events_took_the_first_sequences() {
        let bus = AgentEventBus::new(8);
        // Simulate the shared machine-events domain: agent observations start at 4.
        *bus.local_sequence.lock() = 3;
        let first = bus.publish(draft("s", AgentControlState::Idle));
        *bus.local_sequence.lock() = first.sequence + 5;
        let second = bus.publish(draft("s", AgentControlState::Working));
        match bus.replay(0) {
            AgentReplay::Events { events, .. } => {
                assert_eq!(events.iter().map(|event| event.sequence).collect::<Vec<_>>(), vec![first.sequence, second.sequence]);
            }
            AgentReplay::Gap { .. } => panic!("nothing was evicted, so a cursor of 0 is not a gap"),
        }
        match bus.replay(first.sequence + 2) {
            AgentReplay::Events { events, .. } => assert_eq!(events.len(), 1),
            AgentReplay::Gap { .. } => panic!("a hole made by other machine events is not a gap"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn head_covers_a_sequence_issued_before_its_reading_is_logged() {
        let bus = AgentEventBus::new(8);
        let logged = bus.publish(draft("s", AgentControlState::Idle));
        // A reading took its sequence but `publish` has not logged it yet.
        let in_flight = bus.allocate_sequence(&draft("s", AgentControlState::Working));
        // The domain also moved ahead of the log for other machine events.
        *bus.local_sequence.lock() = in_flight + 2;
        let head = bus.head();
        assert_eq!(bus.revision(), logged.sequence);
        assert!(head > bus.revision());
        assert!(in_flight <= head);

        bus.record(in_flight, draft("s", AgentControlState::Working));
        match bus.replay(head) {
            AgentReplay::Events { events, .. } => {
                assert!(events.is_empty(), "a reading at or before head is not newer: {events:?}")
            }
            AgentReplay::Gap { .. } => panic!("head is a valid cursor"),
        }
        let hit = wait_for(
            &bus,
            "s",
            "working",
            head,
            None,
            std::time::Duration::from_secs(1),
        )
        .await;
        assert!(hit.is_none(), "the in-flight reading must not satisfy a wait from head");
    }

    #[test]
    fn the_slim_payload_has_no_prompt_body() {
        let observation = AgentObservation {
            sequence: 1,
            session_id: "s".into(),
            state: AgentControlState::Blocked,
            state_source: AgentStateSource::Screen,
            tier: AgentTier::Screen,
            prompt_id: Some("p".into()),
            kind: Some("approval".into()),
            screen_revision: Some("rev".into()),
        };
        let slim = observation.slim();
        assert_eq!(slim["sessionId"], "s");
        assert_eq!(slim["promptId"], "p");
        assert_eq!(slim["kind"], "approval");
        assert!(slim.get("body").is_none());
        assert!(slim.get("text").is_none());
        assert!(slim.get("screen").is_none());
    }

    #[test]
    fn quiet_time_makes_a_screen_only_unknown_idle_and_leaves_a_provider_alone() {
        let quiet = classify_screen(None, 80, 24, "hello", true);
        assert_eq!(quiet.state, AgentControlState::Idle);
        assert_eq!(quiet.state_source, AgentStateSource::Screen);
        let fresh = classify_screen(None, 80, 24, "hello", false);
        assert_eq!(fresh.state, AgentControlState::Unknown);

        let provider = ProviderReport {
            agent: Some("omo".into()),
            state: "working".into(),
            self_reported: true,
            detail: None,
        };
        let held = classify_screen(Some(&provider), 80, 24, "hello", true);
        assert_eq!(held.state, AgentControlState::Working);
        assert_eq!(held.state_source, AgentStateSource::Provider);
        assert_eq!(held.tier, AgentTier::Native);
    }

    #[test]
    fn a_prompt_on_the_screen_stays_blocked_through_the_quiet_timer() {
        let screen =
            include_str!("../remote/reference_chat/fixtures/claude/prompt/approval-command.screen");
        let reading = classify_screen(None, 100, 30, screen, true);
        assert_eq!(reading.state, AgentControlState::Blocked);
        assert!(reading.prompt_id.is_some(), "the approval card has an id");
        assert_eq!(reading.kind.as_deref(), Some("approval"));
    }

    #[test]
    fn only_other_input_on_the_same_pane_after_the_cursor_is_foreign() {
        let bus = AgentEventBus::new(8);
        bus.note_input("pane", Some("before"));
        let cursor = bus.input_cursor();
        assert!(!bus.foreign_input_since("pane", cursor, "mine"), "earlier input is not foreign");
        bus.note_input("pane", Some("mine"));
        bus.note_input("other-pane", Some("theirs"));
        assert!(!bus.foreign_input_since("pane", cursor, "mine"));
        bus.note_input("pane", None);
        assert!(bus.foreign_input_since("pane", cursor, "mine"), "untagged input is foreign");

        // Marks that fell out of the bounded ledger after the cursor count as foreign.
        let cursor = bus.input_cursor();
        for _ in 0..=INPUT_MARKS_PER_SESSION {
            bus.note_input("busy", Some("mine"));
        }
        assert!(
            bus.foreign_input_since("busy", cursor, "mine"),
            "a mark evicted after the cursor can no longer be attributed"
        );
    }
}
