//! Agent-control observations and the replay log a supervisor waits on.
//!
//! The sequence domain is the machine-events sequence when that stream is attached, so a change
//! the events socket already carries is the same number `agents events` and `agents wait` use.
//! The log keeps a bounded tail. A cursor that falls out of the tail is a gap plus a snapshot of
//! the latest observation per session, the same recovery `inventoryInvalidated` asks for. The
//! prompt body never enters the log: it is fetched with `agents read`.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::broadcast;

use super::{derive_status, AgentControlState, AgentStateSource, AgentTier, ProviderReport};

/// Machine-events type for one slim agent-control change.
pub const AGENT_CONTROL_EVENT: &str = "agentControlChanged";

/// Events-socket frame telling a remote client that a session's prompt card may have changed.
pub const AGENT_PROMPT_CHANGED_FRAME: &str = "agentPromptChanged";

/// How many sessions one socket's prompt filter remembers. The oldest is forgotten first: a
/// forgotten reading only costs one spare frame, a forgotten verdict one more ownership read.
pub const PROMPT_FILTER_SESSIONS: usize = 256;

/// Per-socket memory of the prompt and screen each session last showed, so a state-only change
/// sends no frame. A session the socket has not seen yet always sends one: a spare refetch is
/// cheaper than a card left open on another device. It also caches whether the socket's surface
/// may see a session at all, so the ownership read runs once per session.
#[derive(Default)]
pub struct PromptChangeFilter {
    seen: HashMap<String, (Option<String>, Option<String>)>,
    exposure: HashMap<String, bool>,
    order: VecDeque<String>,
}

impl PromptChangeFilter {
    /// The cached verdict for whether this socket may see `session_id`.
    pub fn exposure(&self, session_id: &str) -> Option<bool> {
        self.exposure.get(session_id).copied()
    }

    pub fn remember_exposure(&mut self, session_id: &str, exposed: bool) {
        self.track(session_id);
        self.exposure.insert(session_id.to_string(), exposed);
    }

    fn track(&mut self, session_id: &str) {
        if self.seen.contains_key(session_id) || self.exposure.contains_key(session_id) {
            return;
        }
        self.order.push_back(session_id.to_string());
        while self.order.len() > PROMPT_FILTER_SESSIONS {
            if let Some(oldest) = self.order.pop_front() {
                self.seen.remove(&oldest);
                self.exposure.remove(&oldest);
            }
        }
    }

    /// The `agentPromptChanged` frame for this observation, if its prompt or screen moved.
    pub fn frame(&mut self, observation: &AgentObservation) -> Option<String> {
        let reading = (
            observation.prompt_id.clone(),
            observation.screen_revision.clone(),
        );
        if self.seen.get(&observation.session_id) == Some(&reading) {
            return None;
        }
        self.track(&observation.session_id);
        self.seen.insert(observation.session_id.clone(), reading);
        Some(
            json!({
                "type": AGENT_PROMPT_CHANGED_FRAME,
                "sessionId": observation.session_id,
                "promptId": observation.prompt_id,
                "screenRevision": observation.screen_revision,
            })
            .to_string(),
        )
    }
}

/// The backend session a history frame names, for the same exposure check as prompt pushes.
///
/// A frame that is not the contract object has no session. Callers that must hide machine-only
/// sessions treat that as not exposed.
pub fn reference_history_frame_session(frame: &str) -> Option<String> {
    let value: Value = serde_json::from_str(frame).ok()?;
    let session = value.get("target")?.get("backendSessionId")?.as_str()?;
    if session.is_empty() {
        return None;
    }
    Some(session.to_string())
}

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
    /// Epoch ms when the session entered `state`; unchanged while later readings keep it.
    pub state_since_ms: Option<i64>,
    /// Epoch ms when `screen_revision` last changed.
    pub screen_changed_at_ms: Option<i64>,
    /// Whether the owner acknowledged the session's current episode (its open prompt, or the
    /// completion it last reached). The bus stamps it from its read store; a draft never has it.
    #[serde(default)]
    pub read: bool,
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
            "stateSince": self.state_since_ms,
            "screenChangedAt": self.screen_changed_at_ms,
            "read": self.read,
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
    /// Stamps a previous daemon run persisted, until this run's first reading of the session
    /// replaces them. Never part of `latest`, `snapshot` or a replay: a seed is not a reading.
    seeds: HashMap<String, SessionStamps>,
    /// The read store: per session, the current attention episode and whether it was
    /// acknowledged. Kept apart from drafts so an observer tick never undoes an acknowledgement.
    reads: HashMap<String, ReadMark>,
    capacity: usize,
}

/// One session's current attention episode and whether the owner acknowledged it.
///
/// The episode is `prompt:<promptId>` while a prompt is open, and `done:<stateSince>` from the
/// moment a session leaves `working` for `idle` or `unknown`. A new episode starts unread; an
/// acknowledgement marks the current one read and never answers or resolves the prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadMark {
    pub episode: Option<String>,
    pub read: bool,
}

/// What a session's state and screen timestamps depend on: the state and screen revision they
/// were measured against, so a later reading can tell whether each one changed. This is what
/// the journal keeps per session across a daemon restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStamps {
    pub state: AgentControlState,
    pub screen_revision: Option<String>,
    pub state_since_ms: Option<i64>,
    pub screen_changed_at_ms: Option<i64>,
    /// The session's read mark, journaled with its stamps so a restart or handover keeps an
    /// acknowledgement. Absent in journals written before read state existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<ReadMark>,
}

impl SessionStamps {
    fn of(observation: &AgentObservation, read: Option<&ReadMark>) -> Self {
        Self {
            state: observation.state,
            screen_revision: observation.screen_revision.clone(),
            state_since_ms: observation.state_since_ms,
            screen_changed_at_ms: observation.screen_changed_at_ms,
            read: read.cloned(),
        }
    }
}

/// Receives a session's stamps whenever a reading changes them.
pub type StampSink = Arc<dyn Fn(&str, &SessionStamps) + Send + Sync>;

/// Waits for one stamp update, then keeps collecting for `window`, keeping only the newest per
/// session. Returns `None` once the channel is closed with nothing left. A busy session moves its
/// screen revision many times a second; the journal is rewritten and synced per batch, not per
/// reading. The `bool` is true when the channel closed, so the caller writes this batch and stops.
pub fn next_stamp_batch(
    updates: &std::sync::mpsc::Receiver<(String, SessionStamps)>,
    window: Duration,
) -> Option<(std::collections::BTreeMap<String, SessionStamps>, bool)> {
    let mut batch = std::collections::BTreeMap::new();
    let (id, stamps) = updates.recv().ok()?;
    batch.insert(id, stamps);
    let deadline = std::time::Instant::now() + window;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match updates.recv_timeout(left) {
            Ok((id, stamps)) => {
                batch.insert(id, stamps);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Some((batch, false)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Some((batch, true)),
        }
    }
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
    /// The newest mark of any request, raw keys included. Never evicted.
    last: Option<LastMark>,
}

/// When the newest input reached a pane, and how far the observer had got by then.
#[derive(Clone, Copy)]
struct LastMark {
    mark: u64,
    at: tokio::time::Instant,
    /// [`AgentEventBus::head`] read right AFTER the mark was issued (`u64::MAX` until then): a
    /// reading with a higher sequence was allocated after the input went out.
    head: u64,
}

/// Retained agent-control observations. Publishing also forwards a slim machine event when the
/// machine stream is attached.
pub struct AgentEventBus {
    log: Mutex<Log>,
    stamp_sink: Mutex<Option<StampSink>>,
    tx: broadcast::Sender<AgentObservation>,
    /// Advisory `referenceHistoryChanged` frames. A relay forwards these bytes opaquely.
    history_tx: broadcast::Sender<String>,
    machine_events: Mutex<Option<Arc<crate::remote::machine_events::MachineEvents>>>,
    local_sequence: Mutex<u64>,
    inputs: Mutex<InputLedger>,
    /// This bus's observer epoch. Sequences restart with the daemon, so a sequence is only
    /// comparable with others of the same epoch; run ids and wait replies carry it.
    instance: String,
    /// Epoch ms clock for the state and screen timestamps; tests inject a fixed one.
    now_ms: Box<dyn Fn() -> i64 + Send + Sync>,
}

fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// The (state since, screen changed at) pair for `draft` against the session's previous
/// reading: an unchanged state or screen revision keeps its time, a changed one takes `now`.
/// With no reading yet in this run, the stamps a previous run persisted stand in for it, so a
/// restart does not move either time.
fn stamps(log: &Log, draft: &ObservationDraft, now: i64) -> (Option<i64>, Option<i64>) {
    let previous = effective_stamps(log, &draft.session_id);
    let state_since = match &previous {
        Some(seen) if seen.state == draft.state => seen.state_since_ms,
        _ => Some(now),
    };
    let screen_changed_at = match &previous {
        Some(seen) if seen.screen_revision == draft.screen_revision => seen.screen_changed_at_ms,
        _ => Some(now),
    };
    (state_since, screen_changed_at)
}

/// The session's newest stamps: its latest reading in this run, else a persisted seed.
fn effective_stamps(log: &Log, session_id: &str) -> Option<SessionStamps> {
    log.latest
        .get(session_id)
        .map(|seen| SessionStamps::of(seen, log.reads.get(session_id)))
        .or_else(|| log.seeds.get(session_id).cloned())
}

/// The episode-key prefix of a completion, followed by the edge's `state_since`.
const DONE_EPISODE: &str = "done:";

/// The episode a reading opens, if it opens one: an open prompt is its own episode, and a
/// `working` -> `idle`/`unknown` edge is a completion keyed by when it happened. Any other
/// reading continues the current episode.
fn opened_episode(
    previous: Option<AgentControlState>,
    draft: &ObservationDraft,
    state_since_ms: Option<i64>,
) -> Option<String> {
    if let Some(prompt_id) = &draft.prompt_id {
        return Some(format!("prompt:{prompt_id}"));
    }
    let done = matches!(draft.state, AgentControlState::Idle | AgentControlState::Unknown);
    if done && previous == Some(AgentControlState::Working) {
        return Some(format!("{DONE_EPISODE}{}", state_since_ms.unwrap_or_default()));
    }
    None
}

/// The read mark `draft` leaves: a new episode starts unread, a continued one keeps its mark.
/// `ack` marks the resulting episode read.
fn next_read(log: &Log, draft: &ObservationDraft, state_since_ms: Option<i64>, ack: bool) -> Option<ReadMark> {
    let previous = effective_stamps(log, &draft.session_id).map(|seen| seen.state);
    let current = log.reads.get(&draft.session_id);
    let mark = match opened_episode(previous, draft, state_since_ms) {
        // A done edge always leaves `working`, so it is a new completion even when its time
        // repeats the last one's; a prompt is new only under a new id.
        Some(episode)
            if episode.starts_with(DONE_EPISODE)
                || current.and_then(|mark| mark.episode.as_ref()) != Some(&episode) =>
        {
            Some(ReadMark { episode: Some(episode), read: false })
        }
        _ => current.cloned(),
    };
    if !ack {
        return mark;
    }
    Some(ReadMark { episode: mark.and_then(|mark| mark.episode), read: true })
}

impl AgentEventBus {
    pub fn new(capacity: usize) -> Self {
        Self::with_clock(capacity, Box::new(system_now_ms))
    }

    fn with_clock(capacity: usize, now_ms: Box<dyn Fn() -> i64 + Send + Sync>) -> Self {
        let capacity = capacity.max(1);
        let (tx, _) = broadcast::channel(capacity);
        let (history_tx, _) = broadcast::channel(64);
        Self {
            log: Mutex::new(Log {
                revision: 0,
                evicted_through: 0,
                retained: VecDeque::new(),
                latest: HashMap::new(),
                seeds: HashMap::new(),
                reads: HashMap::new(),
                capacity,
            }),
            stamp_sink: Mutex::new(None),
            tx,
            history_tx,
            machine_events: Mutex::new(None),
            local_sequence: Mutex::new(0),
            inputs: Mutex::new(InputLedger::default()),
            instance: uuid::Uuid::new_v4().simple().to_string(),
            now_ms,
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

    /// Subscribe before a history frame is published. A lagged receiver only loses hints.
    pub fn subscribe_history(&self) -> broadcast::Receiver<String> {
        self.history_tx.subscribe()
    }

    /// Emit the contract frame unchanged. No path, content, or model is added here, and a
    /// relay is not asked to interpret it: both event sockets send these bytes as-is.
    pub fn publish_reference_history(
        &self,
        event: &crate::remote::reference_chat::types::ReferenceHistoryEvent,
    ) {
        let Ok(frame) = serde_json::to_string(event) else {
            return;
        };
        let _ = self.history_tx.send(frame);
    }

    pub fn latest(&self, session_id: &str) -> Option<AgentObservation> {
        self.log.lock().latest.get(session_id).cloned()
    }

    /// The completion episode of the session's last turn: `Some(state_since)` of the reading
    /// that moved it from working to idle or unknown, until it works again or a prompt opens a
    /// new episode. Read from the read store, so a completion and its read mark are one episode.
    pub fn completion(&self, session_id: &str) -> Option<Option<i64>> {
        let log = self.log.lock();
        if effective_stamps(&log, session_id).is_some_and(|seen| seen.state == AgentControlState::Working) {
            return None;
        }
        let since = log.reads.get(session_id)?.episode.as_deref()?.strip_prefix(DONE_EPISODE)?;
        Some(since.parse().ok())
    }

    /// Loads the stamps a previous run persisted. Call before the observers start: a seed only
    /// stands in for a session's first reading of this run, and a session with no seed (and no
    /// reading) has no stamps.
    pub fn seed_stamps(&self, seeds: impl IntoIterator<Item = (String, SessionStamps)>) {
        let mut log = self.log.lock();
        for (session_id, stamps) in seeds {
            if !log.latest.contains_key(&session_id) {
                if let Some(read) = &stamps.read {
                    log.reads.entry(session_id.clone()).or_insert_with(|| read.clone());
                }
                log.seeds.insert(session_id, stamps);
            }
        }
    }

    /// Whether the owner acknowledged the session's current episode. A session with no episode
    /// yet, or one this bus never saw, is unread.
    pub fn read_state(&self, session_id: &str) -> bool {
        self.log.lock().reads.get(session_id).is_some_and(|mark| mark.read)
    }

    /// Marks the session's current episode read and republishes its latest reading under a new
    /// sequence, so every events client sees the flip. Never answers or resolves a prompt: a new
    /// prompt id or a new completion opens an unread episode again. `None` when the session has
    /// no reading yet (the mark is still kept for its first one).
    pub fn acknowledge(&self, session_id: &str) -> Option<AgentObservation> {
        let latest = {
            let mut log = self.log.lock();
            let latest = log.latest.get(session_id).cloned();
            if latest.is_none() {
                log.reads
                    .entry(session_id.to_string())
                    .or_insert(ReadMark { episode: None, read: false })
                    .read = true;
            }
            latest
        }?;
        let draft = ObservationDraft {
            session_id: latest.session_id,
            state: latest.state,
            state_source: latest.state_source,
            tier: latest.tier,
            prompt_id: latest.prompt_id,
            kind: latest.kind,
            screen_revision: latest.screen_revision,
            workspace_id: None,
        };
        let now = (self.now_ms)();
        let sequence = self.allocate(&draft, now, true);
        Some(self.record_read(sequence, draft, now, true))
    }

    /// Installs the receiver of stamp changes (the journal writer). It runs on the publishing
    /// thread, outside the bus lock, and must not block.
    pub fn set_stamp_sink(&self, sink: StampSink) {
        *self.stamp_sink.lock() = Some(sink);
    }

    /// Drops the installed stamp sink, which closes the writer's queue so the writer drains what
    /// it holds and ends. The gateway calls it to retire the writer when it is dropped.
    pub(crate) fn clear_stamp_sink(&self) {
        *self.stamp_sink.lock() = None;
    }

    /// The session's newest stamps: its latest reading, else its persisted seed. An entry listed
    /// before the observer's first reading of this run still shows the real times.
    pub fn stamps(&self, session_id: &str) -> Option<SessionStamps> {
        effective_stamps(&self.log.lock(), session_id)
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
        let at = tokio::time::Instant::now();
        let mark = {
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
            session.last = Some(LastMark {
                mark,
                at,
                head: u64::MAX,
            });
            mark
        };
        // Read after the mark exists, so a reading allocated before the input went out is never
        // newer than it. Until this lands the mark counts as unobserved, which is conservative.
        let head = self.head();
        let mut ledger = self.inputs.lock();
        if let Some(last) = ledger
            .sessions
            .get_mut(session_id)
            .and_then(|session| session.last.as_mut())
            .filter(|last| last.mark == mark)
        {
            last.head = head;
        }
    }

    /// Whether the newest input that reached `session_id` (any request, raw keys included) can
    /// no longer be starting a turn the observer has not published: a reading with a sequence
    /// above `reading` was allocated after it, and it is at least `horizon` old. The observer
    /// publishes a pane's reading only once its output settles and calls a quiet screen idle
    /// only after its quiet timer, so an idle reading taken earlier may predate the turn the
    /// input started. A pane no input reached is settled.
    pub fn input_observed(&self, session_id: &str, reading: u64, horizon: Duration) -> bool {
        let ledger = self.inputs.lock();
        let Some(last) = ledger.sessions.get(session_id).and_then(|session| session.last) else {
            return true;
        };
        reading > last.head && last.at.elapsed() >= horizon
    }

    /// When the newest input that reached `session_id` becomes `horizon` old, if any did.
    pub fn input_settles_at(&self, session_id: &str, horizon: Duration) -> Option<tokio::time::Instant> {
        let ledger = self.inputs.lock();
        ledger
            .sessions
            .get(session_id)
            .and_then(|session| session.last)
            .map(|last| last.at + horizon)
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
        let now = (self.now_ms)();
        let sequence = self.allocate_sequence(&draft, now);
        self.record(sequence, draft, now)
    }

    /// Log a reading under a sequence `allocate_sequence` already issued and wake waiters.
    fn record(&self, sequence: u64, draft: ObservationDraft, now: i64) -> AgentObservation {
        self.record_read(sequence, draft, now, false)
    }

    /// [`Self::record`], with `ack` marking the reading's episode read before it is logged, so
    /// the stamp sink journals the flip.
    fn record_read(&self, sequence: u64, draft: ObservationDraft, now: i64, ack: bool) -> AgentObservation {
        let observation;
        let changed_stamps;
        {
            let mut log = self.log.lock();
            let (state_since_ms, screen_changed_at_ms) = stamps(&log, &draft, now);
            let previous = effective_stamps(&log, &draft.session_id);
            let read = next_read(&log, &draft, state_since_ms, ack);
            observation = AgentObservation {
                sequence,
                session_id: draft.session_id,
                state: draft.state,
                state_source: draft.state_source,
                tier: draft.tier,
                prompt_id: draft.prompt_id,
                kind: draft.kind,
                screen_revision: draft.screen_revision,
                state_since_ms,
                screen_changed_at_ms,
                read: read.as_ref().is_some_and(|mark| mark.read),
            };
            if let Some(read) = &read {
                log.reads.insert(observation.session_id.clone(), read.clone());
            }
            log.revision = log.revision.max(sequence);
            let current = SessionStamps::of(&observation, read.as_ref());
            changed_stamps = (previous.as_ref() != Some(&current)).then_some(current);
            log.seeds.remove(&observation.session_id);
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
        if let Some(changed) = changed_stamps {
            let sink = self.stamp_sink.lock().clone();
            if let Some(sink) = sink {
                sink(&observation.session_id, &changed);
            }
        }
        observation
    }

    fn allocate_sequence(&self, draft: &ObservationDraft, now: i64) -> u64 {
        self.allocate(draft, now, false)
    }

    fn allocate(&self, draft: &ObservationDraft, now: i64, ack: bool) -> u64 {
        if let Some(events) = self.machine_events.lock().clone() {
            let (state_since_ms, screen_changed_at_ms, read) = {
                let log = self.log.lock();
                let (state_since_ms, screen_changed_at_ms) = stamps(&log, draft, now);
                let read = next_read(&log, draft, state_since_ms, ack).is_some_and(|mark| mark.read);
                (state_since_ms, screen_changed_at_ms, read)
            };
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
                    state_since_ms,
                    screen_changed_at_ms,
                    read,
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
    let agent = crate::agent_detect::resolve_agent_identity(
        provider.and_then(|report| report.agent.as_deref()),
        detection.as_ref().map(|found| found.manifest_id.as_str()),
    );
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
    fn the_prompt_filter_forgets_the_oldest_session_past_its_bound() {
        let mut filter = PromptChangeFilter::default();
        let observation = |session: String| AgentObservation {
            sequence: 1,
            session_id: session,
            state: AgentControlState::Blocked,
            state_source: AgentStateSource::Screen,
            tier: AgentTier::Screen,
            prompt_id: Some("p".into()),
            kind: None,
            screen_revision: Some("rev".into()),
            state_since_ms: None,
            screen_changed_at_ms: None,
            read: false,
        };
        filter.remember_exposure("s0", true);
        for index in 0..=PROMPT_FILTER_SESSIONS {
            assert!(filter.frame(&observation(format!("s{index}"))).is_some());
        }
        assert_eq!(filter.order.len(), PROMPT_FILTER_SESSIONS);
        assert!(filter.seen.len() <= PROMPT_FILTER_SESSIONS);
        assert_eq!(
            filter.exposure("s0"),
            None,
            "the oldest session is forgotten"
        );
        // A forgotten session's unchanged reading sends one spare frame; a remembered one none.
        assert!(filter.frame(&observation("s0".into())).is_some());
        assert!(filter
            .frame(&observation(format!("s{PROMPT_FILTER_SESSIONS}")))
            .is_none());
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
        let in_flight = bus.allocate_sequence(&draft("s", AgentControlState::Working), 0);
        // The domain also moved ahead of the log for other machine events.
        *bus.local_sequence.lock() = in_flight + 2;
        let head = bus.head();
        assert_eq!(bus.revision(), logged.sequence);
        assert!(head > bus.revision());
        assert!(in_flight <= head);

        bus.record(in_flight, draft("s", AgentControlState::Working), 0);
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

    fn clocked_bus() -> (AgentEventBus, Arc<std::sync::atomic::AtomicI64>) {
        let clock = Arc::new(std::sync::atomic::AtomicI64::new(1_000));
        let reader = Arc::clone(&clock);
        let bus = AgentEventBus::with_clock(
            8,
            Box::new(move || reader.load(std::sync::atomic::Ordering::SeqCst)),
        );
        (bus, clock)
    }

    fn revised(session: &str, state: AgentControlState, revision: &str) -> ObservationDraft {
        let mut reading = draft(session, state);
        reading.screen_revision = Some(revision.into());
        reading
    }

    #[test]
    fn an_unchanged_revision_keeps_screen_changed_at() {
        let (bus, clock) = clocked_bus();
        let first = bus.publish(draft("s", AgentControlState::Working));
        assert_eq!(first.screen_changed_at_ms, Some(1_000));
        clock.store(5_000, std::sync::atomic::Ordering::SeqCst);
        let mut again = draft("s", AgentControlState::Working);
        again.prompt_id = Some("p".into());
        let second = bus.publish(again);
        assert_eq!(second.screen_changed_at_ms, Some(1_000));
        assert_eq!(second.state_since_ms, Some(1_000));
        assert_eq!(second.slim()["screenChangedAt"], 1_000);
    }

    #[test]
    fn a_state_change_resets_state_since() {
        let (bus, clock) = clocked_bus();
        bus.publish(draft("s", AgentControlState::Working));
        clock.store(5_000, std::sync::atomic::Ordering::SeqCst);
        let idle = bus.publish(draft("s", AgentControlState::Idle));
        assert_eq!(idle.state_since_ms, Some(5_000));
        assert_eq!(idle.screen_changed_at_ms, Some(1_000));
        assert_eq!(idle.slim()["stateSince"], 5_000);
        assert_eq!(bus.latest("s").and_then(|seen| seen.state_since_ms), Some(5_000));
    }

    #[test]
    fn a_revision_change_in_the_same_state_moves_only_screen_changed_at() {
        let (bus, clock) = clocked_bus();
        bus.publish(revised("s", AgentControlState::Working, "r1"));
        clock.store(7_000, std::sync::atomic::Ordering::SeqCst);
        let next = bus.publish(revised("s", AgentControlState::Working, "r2"));
        assert_eq!(next.state_since_ms, Some(1_000));
        assert_eq!(next.screen_changed_at_ms, Some(7_000));
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
            state_since_ms: Some(1_000),
            screen_changed_at_ms: Some(2_000),
            read: false,
        };
        let slim = observation.slim();
        assert_eq!(slim["stateSince"], 1_000);
        assert_eq!(slim["screenChangedAt"], 2_000);
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
            outcome: None,
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

    #[tokio::test(start_paused = true)]
    async fn an_input_is_observed_only_by_a_later_reading_past_the_horizon() {
        let bus = AgentEventBus::new(8);
        let horizon = std::time::Duration::from_secs(3);
        assert!(bus.input_observed("pane", 0, horizon), "a pane no input reached is settled");
        assert!(bus.input_settles_at("pane", horizon).is_none());

        let earlier = bus.publish(draft("pane", AgentControlState::Idle));
        let typed = tokio::time::Instant::now();
        bus.note_input("pane", Some("theirs"));
        assert_eq!(bus.input_settles_at("pane", horizon), Some(typed + horizon));
        assert!(!bus.input_observed("pane", earlier.sequence, horizon), "a reading from before");
        let later = bus.publish(draft("pane", AgentControlState::Working));
        assert!(!bus.input_observed("pane", later.sequence, horizon), "within the horizon");
        tokio::time::advance(horizon).await;
        assert!(!bus.input_observed("pane", earlier.sequence, horizon), "still from before");
        assert!(bus.input_observed("pane", later.sequence, horizon));
        assert!(bus.input_observed("other-pane", 0, horizon), "per pane");
    }

    fn seed(state: AgentControlState, revision: &str, since: i64, changed: i64) -> SessionStamps {
        SessionStamps {
            state,
            screen_revision: Some(revision.into()),
            state_since_ms: Some(since),
            screen_changed_at_ms: Some(changed),
            read: None,
        }
    }

    #[test]
    fn a_fresh_bus_keeps_seeded_stamps_when_state_and_revision_are_unchanged() {
        let (bus, clock) = clocked_bus();
        bus.seed_stamps([("s".to_string(), seed(AgentControlState::Working, "rev", 100, 200))]);
        // The restart happened at 1_000 (the bus clock), long after both seeded times.
        clock.store(9_000, std::sync::atomic::Ordering::SeqCst);
        let first = bus.publish(draft("s", AgentControlState::Working));
        assert_eq!(first.state_since_ms, Some(100));
        assert_eq!(first.screen_changed_at_ms, Some(200), "downtime is not hidden by a reset");
        assert_eq!(first.slim()["screenChangedAt"], 200);
    }

    #[test]
    fn a_seed_stands_in_before_the_first_reading_and_is_not_a_reading() {
        let (bus, _clock) = clocked_bus();
        bus.seed_stamps([("s".to_string(), seed(AgentControlState::Idle, "rev", 100, 200))]);
        assert_eq!(bus.stamps("s"), Some(seed(AgentControlState::Idle, "rev", 100, 200)));
        assert!(bus.latest("s").is_none() && bus.snapshot().is_empty(), "a seed is not an observation");
        assert_eq!(bus.revision(), 0);
        assert_eq!(bus.stamps("unknown-session"), None, "a session nobody persisted stays None");
    }

    #[test]
    fn a_changed_state_after_restart_moves_only_state_since() {
        let (bus, clock) = clocked_bus();
        bus.seed_stamps([("s".to_string(), seed(AgentControlState::Working, "rev", 100, 200))]);
        clock.store(5_000, std::sync::atomic::Ordering::SeqCst);
        let idle = bus.publish(draft("s", AgentControlState::Idle));
        assert_eq!(idle.state_since_ms, Some(5_000));
        assert_eq!(idle.screen_changed_at_ms, Some(200));
    }

    #[test]
    fn a_changed_revision_after_restart_moves_only_screen_changed_at() {
        let (bus, clock) = clocked_bus();
        bus.seed_stamps([("s".to_string(), seed(AgentControlState::Working, "old", 100, 200))]);
        clock.store(6_000, std::sync::atomic::Ordering::SeqCst);
        let next = bus.publish(revised("s", AgentControlState::Working, "new"));
        assert_eq!(next.state_since_ms, Some(100));
        assert_eq!(next.screen_changed_at_ms, Some(6_000));
    }

    #[test]
    fn an_unseeded_session_after_restart_starts_at_the_observation_time() {
        let (bus, _clock) = clocked_bus();
        bus.seed_stamps([("other".to_string(), seed(AgentControlState::Working, "rev", 100, 200))]);
        let first = bus.publish(draft("s", AgentControlState::Working));
        assert_eq!(first.state_since_ms, Some(1_000));
        assert_eq!(first.screen_changed_at_ms, Some(1_000));
    }

    #[test]
    fn a_seed_never_overrides_a_reading_this_run_already_made() {
        let (bus, _clock) = clocked_bus();
        bus.publish(draft("s", AgentControlState::Working));
        bus.seed_stamps([("s".to_string(), seed(AgentControlState::Working, "rev", 1, 2))]);
        assert_eq!(bus.stamps("s").and_then(|seen| seen.state_since_ms), Some(1_000));
    }

    #[test]
    fn the_sink_hears_only_changed_stamps_with_the_state_and_revision_they_measure() {
        let (bus, clock) = clocked_bus();
        let heard = Arc::new(Mutex::new(Vec::<(String, SessionStamps)>::new()));
        let sink_heard = Arc::clone(&heard);
        bus.set_stamp_sink(Arc::new(move |id, stamps| sink_heard.lock().push((id.to_string(), stamps.clone()))));
        bus.publish(revised("s", AgentControlState::Working, "r1"));
        // Same state, revision and stamps, and the same prompt again: nothing to persist.
        let mut prompt_only = revised("s", AgentControlState::Working, "r1");
        prompt_only.prompt_id = Some("p".into());
        bus.publish(prompt_only.clone());
        bus.publish(prompt_only);
        clock.store(4_000, std::sync::atomic::Ordering::SeqCst);
        bus.publish(revised("s", AgentControlState::Working, "r2"));
        let heard = heard.lock();
        // The new prompt opened an unread episode, which the journal keeps with the stamps.
        let episode = Some(ReadMark { episode: Some("prompt:p".into()), read: false });
        assert_eq!(heard.len(), 3, "{heard:?}");
        assert_eq!(heard[0].1, seed(AgentControlState::Working, "r1", 1_000, 1_000));
        assert_eq!(heard[1].1, SessionStamps { read: episode.clone(), ..seed(AgentControlState::Working, "r1", 1_000, 1_000) });
        assert_eq!(heard[2].1, SessionStamps { read: episode, ..seed(AgentControlState::Working, "r2", 1_000, 4_000) });
    }

    #[test]
    fn an_unchanged_seeded_session_does_not_rewrite_its_stamps_on_the_first_reading() {
        let (bus, _clock) = clocked_bus();
        let heard = Arc::new(Mutex::new(0usize));
        let sink_heard = Arc::clone(&heard);
        bus.set_stamp_sink(Arc::new(move |_, _| *sink_heard.lock() += 1));
        bus.seed_stamps([("s".to_string(), seed(AgentControlState::Working, "rev", 100, 200))]);
        bus.publish(draft("s", AgentControlState::Working));
        assert_eq!(*heard.lock(), 0);
    }

    mod read_state {
        use super::*;

        fn prompt(session: &str, prompt_id: &str) -> ObservationDraft {
            let mut reading = draft(session, AgentControlState::Blocked);
            reading.prompt_id = Some(prompt_id.into());
            reading.kind = Some("question".into());
            reading
        }

        #[test]
        fn a_new_wait_episode_is_unread() {
            let (bus, clock) = clocked_bus();
            bus.publish(draft("s", AgentControlState::Working));
            assert!(!bus.read_state("s"));
            let blocked = bus.publish(prompt("s", "p1"));
            assert!(!blocked.read, "a new prompt is unread");
            assert_eq!(blocked.slim()["read"], false);
            clock.store(2_000, std::sync::atomic::Ordering::SeqCst);
            bus.publish(draft("s", AgentControlState::Working));
            clock.store(3_000, std::sync::atomic::Ordering::SeqCst);
            let done = bus.publish(draft("s", AgentControlState::Idle));
            assert!(!done.read, "a working -> idle edge is a new unread completion");
            assert!(!bus.read_state("s"));
        }

        #[tokio::test]
        async fn acknowledge_marks_read_and_publishes_an_observation() {
            let bus = AgentEventBus::new(8);
            let before = bus.publish(prompt("s", "p1"));
            let mut events = bus.subscribe();
            let acked = bus.acknowledge("s").expect("the session has a reading");
            assert!(acked.read && bus.read_state("s"));
            assert!(acked.sequence > before.sequence, "the flip is a new sequence");
            assert_eq!(events.recv().await.unwrap(), acked, "subscribers hear the flip");
            match bus.replay(before.sequence) {
                AgentReplay::Events { events, .. } => {
                    assert_eq!(events.len(), 1);
                    assert!(events[0].read);
                    assert_eq!(events[0].sequenced()["read"], true);
                }
                AgentReplay::Gap { .. } => panic!("the flip is on the events log"),
            }
            // The observer's next unchanged tick does not undo the acknowledgement.
            assert!(bus.latest("s").unwrap().same_reading(&prompt("s", "p1")));
            assert!(bus.publish(prompt("s", "p1")).read);
            assert!(bus.acknowledge("unknown").is_none());
        }

        #[test]
        fn a_new_prompt_after_ack_is_unread_again() {
            let (bus, clock) = clocked_bus();
            bus.publish(prompt("s", "p1"));
            bus.acknowledge("s");
            assert!(bus.read_state("s"));
            let next = bus.publish(prompt("s", "p2"));
            assert!(!next.read && !bus.read_state("s"), "a new prompt id reopens attention");
            bus.acknowledge("s");
            bus.publish(draft("s", AgentControlState::Working));
            clock.store(9_000, std::sync::atomic::Ordering::SeqCst);
            assert!(!bus.publish(draft("s", AgentControlState::Idle)).read, "a new completion reopens it");
        }

        #[test]
        fn acking_an_open_demand_makes_it_seen_without_resolving_the_prompt() {
            let bus = AgentEventBus::new(8);
            bus.publish(prompt("s", "p1"));
            let acked = bus.acknowledge("s").unwrap();
            assert!(acked.read);
            assert_eq!(acked.state, AgentControlState::Blocked, "still waiting");
            assert_eq!(acked.prompt_id.as_deref(), Some("p1"), "the prompt stays open");
            assert_eq!(acked.kind.as_deref(), Some("question"));
            let repaint = bus.publish(ObservationDraft { screen_revision: Some("r2".into()), ..prompt("s", "p1") });
            assert!(repaint.read, "the same prompt stays seen until a new prompt id");
        }

        #[test]
        fn a_read_mark_survives_a_restart_through_the_journaled_stamps() {
            let bus = AgentEventBus::new(8);
            let heard = Arc::new(Mutex::new(None::<SessionStamps>));
            let sink_heard = Arc::clone(&heard);
            bus.set_stamp_sink(Arc::new(move |_, stamps| *sink_heard.lock() = Some(stamps.clone())));
            bus.publish(prompt("s", "p1"));
            bus.acknowledge("s");
            let persisted = heard.lock().clone().expect("the ack was journaled");
            assert_eq!(persisted.read, Some(ReadMark { episode: Some("prompt:p1".into()), read: true }));
            let restarted = AgentEventBus::new(8);
            restarted.seed_stamps([("s".to_string(), persisted)]);
            assert!(restarted.read_state("s"));
            assert!(restarted.publish(prompt("s", "p1")).read);
            assert!(!restarted.publish(prompt("s", "p2")).read);
        }

        #[test]
        fn an_unread_completion_survives_a_restart_and_a_new_turn_ends_it() {
            let (bus, clock) = clocked_bus();
            let heard = Arc::new(Mutex::new(None::<SessionStamps>));
            let sink_heard = Arc::clone(&heard);
            bus.set_stamp_sink(Arc::new(move |_, stamps| *sink_heard.lock() = Some(stamps.clone())));
            bus.publish(draft("s", AgentControlState::Working));
            clock.store(5_000, std::sync::atomic::Ordering::SeqCst);
            bus.publish(draft("s", AgentControlState::Idle));
            assert_eq!(bus.completion("s"), Some(Some(5_000)));
            let persisted = heard.lock().clone().expect("the completion was journaled");
            let restarted = AgentEventBus::new(8);
            restarted.seed_stamps([("s".to_string(), persisted)]);
            assert_eq!(restarted.completion("s"), Some(Some(5_000)), "a restart keeps the completion");
            assert!(!restarted.read_state("s"), "and keeps it unread");
            restarted.publish(draft("s", AgentControlState::Working));
            assert_eq!(restarted.completion("s"), None, "a new turn ends it");
        }
    }

    #[test]
    fn the_stamp_batcher_keeps_the_newest_per_session_and_reports_a_closed_queue() {
        let (queue, updates) = std::sync::mpsc::channel();
        queue.send(("a".to_string(), seed(AgentControlState::Working, "r1", 1, 1))).unwrap();
        queue.send(("a".to_string(), seed(AgentControlState::Working, "r2", 1, 2))).unwrap();
        queue.send(("b".to_string(), seed(AgentControlState::Idle, "r1", 3, 3))).unwrap();
        drop(queue);
        let (batch, closed) = next_stamp_batch(&updates, Duration::from_secs(5)).expect("a batch");
        assert!(closed, "a disconnected queue ends the writer without waiting out the window");
        assert_eq!(batch.len(), 2);
        assert_eq!(batch["a"].screen_revision.as_deref(), Some("r2"));
        assert!(next_stamp_batch(&updates, Duration::from_secs(5)).is_none());
    }

    #[tokio::test]
    async fn a_subscriber_that_subscribed_first_receives_one_history_frame_without_a_path() {
        use crate::remote::reference_chat::types::{ReferenceHistoryEvent, ReferenceTargetRef};
        use crate::scoped_contracts::{Epoch, TargetRef};

        let bus = AgentEventBus::new(4);
        let mut frames = bus.subscribe_history();
        let event = ReferenceHistoryEvent::ReferenceHistoryChanged {
            target: ReferenceTargetRef::without_provider_session(TargetRef {
                host_id: "host".into(),
                owner_id: "owner".into(),
                epoch: Epoch(3),
                backend_session_id: "sess".into(),
            }),
            change_seq: "7".into(),
        };
        bus.publish_reference_history(&event);
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), frames.recv())
            .await
            .expect("bounded")
            .expect("frame");
        assert_eq!(frame, serde_json::to_string(&event).expect("opaque frame"));
        assert!(!frame.contains("path"));
        assert!(!frame.contains("/tmp"));
        assert_eq!(reference_history_frame_session(&frame).as_deref(), Some("sess"));
        let value: serde_json::Value = serde_json::from_str(&frame).expect("json");
        assert_eq!(value["type"], "referenceHistoryChanged");
        assert_eq!(value["changeSeq"], "7");
        assert!(value.get("path").is_none());
        assert!(value["target"].get("model").is_none());
    }
}
