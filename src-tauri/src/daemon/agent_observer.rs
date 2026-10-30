//! Per-session agent observer.
//!
//! One task per live PTY wakes on output-hub output (debounced) and on agent-state
//! self-reports. It reads the visible screen from the session's daemon screen mirror
//! (`TerminalOutputHub::visible_screen_text`), which already tracks the pane's size, runs the
//! existing screen detectors outside the mirror's lock, and publishes a slim reading when the
//! reading changes. Output bytes are only a wake signal; the observer parses nothing itself.
//! Idle for a screen-only agent is a quiet-time timer, not a poll.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::{broadcast, watch};

use crate::agent_control::events::{classify_screen, AgentEventBus};
use crate::agent_control::ProviderReport;
use crate::daemon::agent_state::AgentStateHub;
use crate::terminal::screen_mirror::VisibleScreen;
use crate::terminal::TerminalService;

/// How long output has to settle, and how long a quiet screen waits before idle.
#[derive(Debug, Clone, Copy)]
pub struct ObserverTiming {
    pub debounce: Duration,
    pub quiet: Duration,
}

impl Default for ObserverTiming {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(300),
            quiet: Duration::from_secs(3),
        }
    }
}

struct Slot {
    generation: u64,
    stop: watch::Sender<bool>,
}

type Slots = Arc<Mutex<HashMap<String, Slot>>>;
type WorkspaceLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Observes every daemon-owned PTY.
pub struct AgentObserverHub {
    terminals: Arc<TerminalService>,
    bus: Arc<AgentEventBus>,
    slots: Slots,
    timing: ObserverTiming,
    sources: Mutex<Option<Arc<AgentStateHub>>>,
    workspace_of: Mutex<Option<WorkspaceLookup>>,
    generation: AtomicU64,
}

impl AgentObserverHub {
    pub fn new(terminals: Arc<TerminalService>, bus: Arc<AgentEventBus>) -> Self {
        Self::with_timing(terminals, bus, ObserverTiming::default())
    }

    pub fn with_timing(
        terminals: Arc<TerminalService>,
        bus: Arc<AgentEventBus>,
        timing: ObserverTiming,
    ) -> Self {
        Self {
            terminals,
            bus,
            slots: Arc::new(Mutex::new(HashMap::new())),
            timing,
            sources: Mutex::new(None),
            workspace_of: Mutex::new(None),
            generation: AtomicU64::new(1),
        }
    }

    pub fn bus(&self) -> Arc<AgentEventBus> {
        Arc::clone(&self.bus)
    }

    pub(crate) fn attach(
        &self,
        states: Arc<AgentStateHub>,
        events: Arc<crate::remote::machine_events::MachineEvents>,
    ) {
        *self.sources.lock() = Some(states);
        self.bus.attach_machine_events(events);
    }

    pub fn set_workspace_lookup(&self, lookup: WorkspaceLookup) {
        *self.workspace_of.lock() = Some(lookup);
    }

    pub fn watched(&self) -> usize {
        self.slots.lock().len()
    }

    /// Start observing `session_id` unless it is already observed. A watch costs no parser of
    /// its own, so every session is observed.
    pub fn watch(&self, session_id: &str) -> bool {
        let mut slots = self.slots.lock();
        if slots.contains_key(session_id) {
            return true;
        }
        let generation = self.generation.fetch_add(1, Ordering::Relaxed);
        let (stop, stop_rx) = watch::channel(false);
        slots.insert(session_id.to_string(), Slot { generation, stop });
        drop(slots);

        let session_id = session_id.to_string();
        // Weak: the observer reads the pane through the service's own output hub, so a strong
        // handle would keep that hub's sender (and with it the whole service and its runtimes)
        // alive for as long as the observer waits on it.
        let terminals = Arc::downgrade(&self.terminals);
        let bus = Arc::clone(&self.bus);
        let sources = self.sources.lock().clone();
        let workspace_of = self.workspace_of.lock().clone();
        let timing = self.timing;
        let slots = Arc::clone(&self.slots);
        tokio::spawn(async move {
            observe_session(
                session_id.clone(),
                terminals,
                bus,
                sources,
                workspace_of,
                timing,
                stop_rx,
            )
            .await;
            let mut slots = slots.lock();
            if slots
                .get(&session_id)
                .is_some_and(|slot| slot.generation == generation)
            {
                slots.remove(&session_id);
            }
        });
        true
    }

    pub fn unwatch(&self, session_id: &str) {
        if let Some(slot) = self.slots.lock().remove(session_id) {
            let _ = slot.stop.send(true);
        }
    }

    /// Observe every PTY the terminal service currently owns.
    pub fn watch_live(&self) {
        for session_id in self.terminals.list_sessions() {
            self.watch(&session_id);
        }
    }
}

fn provider_report(states: &AgentStateHub, session_id: &str) -> Option<ProviderReport> {
    let state = states.current(session_id)?;
    Some(ProviderReport {
        agent: state.agent,
        state: state.state,
        self_reported: state.origin.is_self_reported(),
        detail: state.detail,
    })
}

/// The session's mirrored screen, or `None` once the service or the session is gone.
fn read_screen(terminals: &std::sync::Weak<TerminalService>, session_id: &str) -> Option<VisibleScreen> {
    terminals.upgrade()?.output_hub().visible_screen_text(session_id)
}

fn publish_if_changed(
    bus: &AgentEventBus,
    session_id: &str,
    workspace_of: Option<&WorkspaceLookup>,
    provider: Option<&ProviderReport>,
    screen: &VisibleScreen,
    quiet_elapsed: bool,
) {
    let mut draft = classify_screen(provider, screen.cols, screen.rows, &screen.text, quiet_elapsed);
    draft.session_id = session_id.to_string();
    draft.workspace_id = workspace_of.and_then(|lookup| lookup(session_id));
    let changed = match bus.latest(session_id) {
        Some(previous) => !previous.same_reading(&draft),
        None => true,
    };
    if changed {
        bus.publish(draft);
    }
}

async fn observe_session(
    session_id: String,
    terminals: std::sync::Weak<TerminalService>,
    bus: Arc<AgentEventBus>,
    sources: Option<Arc<AgentStateHub>>,
    workspace_of: Option<WorkspaceLookup>,
    timing: ObserverTiming,
    mut stop: watch::Receiver<bool>,
) {
    let Some(service) = terminals.upgrade() else {
        return;
    };
    // The raw output stream is only a wake signal: the history is dropped and every chunk's
    // bytes are ignored, since the hub's screen mirror has already parsed them.
    let Some((_history, mut output)) = service.output_hub().subscribe(&session_id) else {
        return;
    };
    drop(service);
    let mut agent_rx = sources
        .as_ref()
        .map(|states| states.subscribe(&session_id).receiver);

    let mut dirty = true;
    let mut quiet_done = false;
    let mut debounce_at = tokio::time::Instant::now() + timing.debounce;
    let mut quiet_at = tokio::time::Instant::now() + timing.quiet;

    loop {
        let agent_wait = async {
            match agent_rx.as_mut() {
                Some(receiver) => receiver.recv().await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            changed = stop.changed() => {
                if changed.is_err() || *stop.borrow() {
                    break;
                }
            }
            incoming = output.recv() => {
                match incoming {
                    // A lagged receiver missed only wake-ups; the mirror saw every byte.
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        dirty = true;
                        quiet_done = false;
                        debounce_at = tokio::time::Instant::now() + timing.debounce;
                        quiet_at = tokio::time::Instant::now() + timing.quiet;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            update = agent_wait => {
                match update {
                    Ok(update) if update.state.session_id == session_id => {
                        // Each report is an edge of its own, published now from the report
                        // itself. Sampling the retained state when the debounce fires instead
                        // would fold a turn whose Working and Idle both arrive within one
                        // debounce (a fast turn, or reports queued behind pane output) into a
                        // single Idle, and a delivery would never see that turn start.
                        let Some(screen) = read_screen(&terminals, &session_id) else { break };
                        let provider = ProviderReport {
                            agent: update.state.agent.clone(),
                            state: update.state.state.clone(),
                            self_reported: update.state.origin.is_self_reported(),
                            detail: update.state.detail.clone(),
                        };
                        publish_if_changed(
                            &bus,
                            &session_id,
                            workspace_of.as_ref(),
                            Some(&provider),
                            &screen,
                            false,
                        );
                        quiet_done = false;
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        if let Some(states) = sources.as_ref() {
                            agent_rx = Some(states.subscribe(&session_id).receiver);
                        }
                        dirty = true;
                        debounce_at = tokio::time::Instant::now();
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        agent_rx = None;
                    }
                }
            }
            _ = tokio::time::sleep_until(debounce_at), if dirty => {
                let Some(screen) = read_screen(&terminals, &session_id) else { break };
                let provider = sources.as_ref().and_then(|states| provider_report(states, &session_id));
                publish_if_changed(&bus, &session_id, workspace_of.as_ref(), provider.as_ref(), &screen, false);
                dirty = false;
            }
            _ = tokio::time::sleep_until(quiet_at), if !dirty && !quiet_done => {
                let Some(screen) = read_screen(&terminals, &session_id) else { break };
                let provider = sources.as_ref().and_then(|states| provider_report(states, &session_id));
                publish_if_changed(&bus, &session_id, workspace_of.as_ref(), provider.as_ref(), &screen, true);
                quiet_done = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    use crate::agent_control::{AgentControlState, AgentStateSource};
    use crate::daemon::agent_state::AgentState;
    use crate::daemon::protocol::AgentStateOrigin;

    fn timing() -> ObserverTiming {
        ObserverTiming {
            debounce: Duration::from_millis(20),
            quiet: Duration::from_millis(60),
        }
    }

    fn hub() -> (
        Arc<TerminalService>,
        Arc<AgentEventBus>,
        Arc<AgentObserverHub>,
    ) {
        let terminals = Arc::new(TerminalService::default());
        let bus = Arc::new(AgentEventBus::new(32));
        let observers = Arc::new(AgentObserverHub::with_timing(
            Arc::clone(&terminals),
            Arc::clone(&bus),
            timing(),
        ));
        (terminals, bus, observers)
    }

    async fn until_state(bus: &AgentEventBus, session: &str, state: AgentControlState) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if bus
                .latest(session)
                .is_some_and(|reading| reading.state == state)
            {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    #[tokio::test]
    async fn output_then_quiet_time_ends_idle_and_a_repeat_is_not_republished() {
        let (terminals, bus, observers) = hub();
        let session = "observer-quiet";
        terminals.output_hub().register_session(session);
        terminals
            .output_hub()
            .publish(session, b"hello\r\n".to_vec());
        assert!(observers.watch(session));
        assert!(
            until_state(&bus, session, AgentControlState::Idle).await,
            "quiet time turns a screen-only pane idle"
        );
        let sequence = bus.latest(session).unwrap().sequence;
        assert_eq!(
            bus.latest(session).unwrap().state_source,
            AgentStateSource::Screen
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            bus.latest(session).unwrap().sequence,
            sequence,
            "the same idle reading is not published again when the pane stays quiet"
        );
        observers.unwatch(session);
    }

    #[tokio::test]
    async fn a_provider_report_is_published_without_waiting_for_the_quiet_timer() {
        let (terminals, bus, observers) = hub();
        let states = Arc::new(AgentStateHub::default());
        let session = "observer-provider";
        terminals.output_hub().register_session(session);
        states.publish_canonical(AgentState {
            session_id: session.into(),
            state: "working".into(),
            agent: Some("omo".into()),
            provider_session: None,
            detail: None,
            origin: AgentStateOrigin::Agent,
        });
        observers.attach(
            states,
            Arc::new(crate::remote::machine_events::MachineEvents::new(8)),
        );
        assert!(observers.watch(session));
        assert!(
            until_state(&bus, session, AgentControlState::Working).await,
            "a self-report is a reading on its own"
        );
        let reading = bus.latest(session).unwrap();
        assert_eq!(reading.state_source, AgentStateSource::Provider);
        observers.unwatch(session);
    }

    fn report(session: &str, value: &str) -> AgentState {
        AgentState {
            session_id: session.into(),
            state: value.into(),
            agent: Some("omo".into()),
            provider_session: None,
            detail: None,
            origin: AgentStateOrigin::Agent,
        }
    }

    /// Waits, subscribed before the caller's trigger, for the reading after `after` that
    /// matches `state`, under a bounded deadline.
    async fn next_reading(
        receiver: &mut broadcast::Receiver<crate::agent_control::events::AgentObservation>,
        session: &str,
        state: AgentControlState,
    ) -> crate::agent_control::events::AgentObservation {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let reading = receiver.recv().await.expect("bus open");
                if reading.session_id == session && reading.state == state {
                    return reading;
                }
            }
        })
        .await
        .expect("the reading was published")
    }

    /// A corrective (second) turn on a pane that already ran one: the agent reports Working and
    /// Idle back to back, well inside one debounce. Each report is an edge, so the bus logs the
    /// Working start between the two Idles, where a delivery's cursor finds it.
    #[tokio::test]
    async fn a_second_turn_faster_than_the_debounce_still_logs_its_start() {
        let terminals = Arc::new(TerminalService::default());
        let bus = Arc::new(AgentEventBus::new(32));
        // A debounce far longer than the turn: sampling at the debounce would see only Idle.
        let observers = Arc::new(AgentObserverHub::with_timing(
            Arc::clone(&terminals),
            Arc::clone(&bus),
            ObserverTiming {
                debounce: Duration::from_millis(500),
                quiet: Duration::from_secs(5),
            },
        ));
        let states = Arc::new(AgentStateHub::default());
        let session = "observer-fast-second-turn";
        terminals.output_hub().register_session(session);
        observers.attach(
            Arc::clone(&states),
            Arc::new(crate::remote::machine_events::MachineEvents::new(8)),
        );
        let mut readings = bus.subscribe();
        assert!(observers.watch(session));

        // The first turn, then the pane settles idle.
        states.publish_canonical(report(session, "working"));
        next_reading(&mut readings, session, AgentControlState::Working).await;
        states.publish_canonical(report(session, "idle"));
        let settled = next_reading(&mut readings, session, AgentControlState::Idle).await;

        // The corrective delivery's cursor, then a sub-debounce Working -> Idle turn, with
        // pane output in between (the typed instruction and the turn's own output).
        let cursor = bus.head();
        terminals
            .output_hub()
            .publish(session, b"corrective instruction\r\n".to_vec());
        states.publish_canonical(report(session, "working"));
        terminals
            .output_hub()
            .publish(session, b"turn 2: verify.sh exited 0\r\n".to_vec());
        states.publish_canonical(report(session, "idle"));

        let started = next_reading(&mut readings, session, AgentControlState::Working).await;
        let ended = next_reading(&mut readings, session, AgentControlState::Idle).await;
        assert!(started.sequence > cursor && started.sequence > settled.sequence);
        assert!(ended.sequence > started.sequence);
        let replay = bus.replay(cursor);
        let crate::agent_control::events::AgentReplay::Events { events, .. } = replay else {
            panic!("the cursor is inside the log");
        };
        let start_logged = events.iter().any(|reading| {
            reading.session_id == session
                && reading.sequence > cursor
                && reading.state == AgentControlState::Working
        });
        assert!(
            start_logged,
            "the second turn's start is in the log after the delivery cursor: {events:?}"
        );
        observers.unwatch(session);
    }

    /// Every session is observed now that a watch costs no parser of its own: 80 panes all
    /// reach a reading.
    #[tokio::test]
    async fn eighty_sessions_are_all_observed_and_classified() {
        let (terminals, bus, observers) = hub();
        let mut readings = bus.subscribe();
        let sessions: Vec<String> = (0..80).map(|index| format!("observer-many-{index}")).collect();
        for session in &sessions {
            terminals.output_hub().register_session(session);
            terminals.output_hub().publish(session, format!("{session}\r\n$ ").into_bytes());
            assert!(observers.watch(session));
        }
        assert_eq!(observers.watched(), sessions.len());
        // The bus holds fewer readings than there are sessions, so a lagged receiver is only a
        // wake-up here; the retained latest reading of each session is what is checked.
        let all_idle = || {
            sessions.iter().all(|session| {
                bus.latest(session)
                    .is_some_and(|reading| reading.state == AgentControlState::Idle)
            })
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            while !all_idle() {
                match readings.recv().await {
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => panic!("bus closed"),
                }
            }
        })
        .await
        .expect("every session reached a quiet-time reading");
        for session in &sessions {
            observers.unwatch(session);
        }
    }

    /// The engine the observer used to own: a private parser at the pane size with no
    /// scrollback, read top to bottom. Kept here only as the reference the mirror must match.
    struct OldEngine(crate::native_terminal::NativeTerminal);

    impl OldEngine {
        fn new(cols: u16, rows: u16) -> Self {
            let mut engine = crate::native_terminal::NativeTerminal::new(cols, rows).unwrap();
            engine.set_pty_writes_suppressed(true);
            engine.set_scrollback_limit_lines(Some(0)).unwrap();
            Self(engine)
        }

        fn feed(&mut self, bytes: &[u8]) {
            use crate::native_terminal::TerminalEngine;
            self.0.feed(bytes).unwrap();
        }

        fn resize(&mut self, cols: u16, rows: u16) {
            use crate::native_terminal::TerminalEngine;
            self.0.resize(cols, rows, 8, 16).unwrap();
        }

        fn screen(&self) -> VisibleScreen {
            use crate::native_terminal::TerminalEngine;
            let (cols, rows) = self.0.dimensions().unwrap();
            // The old reader started at row 0, which is the top of the screen only while no
            // scrollback is retained; start past whatever the parser kept so the reference is
            // the visible screen either way.
            let top = self.0.scrollback_rows().unwrap();
            let mut lines: Vec<String> = (top..top + usize::from(rows))
                .map(|row| self.0.screen_row_text(row).unwrap().trim_end().to_string())
                .collect();
            while lines.last().is_some_and(|line| line.is_empty()) {
                lines.pop();
            }
            VisibleScreen {
                text: lines.join("\n"),
                cols,
                rows,
                alt_screen: self.0.is_alternate_screen().unwrap(),
            }
        }
    }

    fn provider() -> ProviderReport {
        ProviderReport {
            agent: Some("claude".into()),
            state: "working".into(),
            self_reported: true,
            detail: None,
        }
    }

    /// The mirror's screen and the old engine's screen are the same text at the same size, and
    /// classify to the same reading with and without a provider report.
    fn assert_same_classification(mirror: &VisibleScreen, old: &VisibleScreen) {
        assert_eq!(mirror, old);
        let provider = provider();
        for report in [None, Some(&provider)] {
            for quiet in [false, true] {
                assert_eq!(
                    classify_screen(report, mirror.cols, mirror.rows, &mirror.text, quiet),
                    classify_screen(report, old.cols, old.rows, &old.text, quiet),
                );
            }
        }
    }

    fn mirrored(cols: u16, rows: u16, session: &str) -> crate::terminal::TerminalOutputHub {
        let hub = crate::terminal::TerminalOutputHub::default();
        hub.register_session(session);
        hub.record_initial_size(session, cols, rows);
        hub
    }

    fn scrolling_output(lines: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for line in 0..lines {
            bytes.extend_from_slice(format!("output line {line}\r\n").as_bytes());
        }
        bytes
    }

    const PERMISSION_SCREEN: &[u8] = b"  Run this command?\r\n  rm -rf build/\r\n\r\n  Do you want to proceed?\r\n  1. Yes  2. No";

    #[test]
    fn mirror_text_classifies_like_the_old_engine_for_the_same_bytes() {
        let chunks: [&[u8]; 3] = [&scrolling_output(60), b"\x1b[31mcolored\x1b[0m\r\n", PERMISSION_SCREEN];
        let session = "observer-equal";
        let hub = mirrored(80, 24, session);
        let mut old = OldEngine::new(80, 24);
        for chunk in chunks {
            hub.publish(session, chunk.to_vec());
            old.feed(chunk);
            let screen = hub.visible_screen_text(session).unwrap();
            assert!(!screen.text.contains("output line 0\n"), "scrollback is not part of the screen");
            assert_same_classification(&screen, &old.screen());
        }
    }

    #[test]
    fn alternate_screen_text_comes_from_the_mirror() {
        let session = "observer-alt";
        let hub = mirrored(80, 24, session);
        let mut old = OldEngine::new(80, 24);
        let mut bytes = scrolling_output(40);
        bytes.extend_from_slice(b"\x1b[?1049h\x1b[H");
        bytes.extend_from_slice(PERMISSION_SCREEN);
        hub.publish(session, bytes.clone());
        old.feed(&bytes);
        let screen = hub.visible_screen_text(session).unwrap();
        assert!(screen.alt_screen);
        assert!(screen.text.starts_with("  Run this command?"), "{:?}", screen.text);
        assert_same_classification(&screen, &old.screen());

        hub.publish(session, b"\x1b[?1049l".to_vec());
        old.feed(b"\x1b[?1049l");
        let screen = hub.visible_screen_text(session).unwrap();
        assert!(!screen.alt_screen);
        assert!(screen.text.contains("output line 39"));
        assert_same_classification(&screen, &old.screen());
    }

    /// The size comes from the mirror, which the hub resizes on every recorded resize. The
    /// output stays within the screen before the grow: growing rows over retained scrollback
    /// pulls history down in the mirror (as in the real pane), which the old engine kept none of.
    #[test]
    fn a_recorded_resize_reaches_the_mirror_text() {
        let session = "observer-resize";
        let hub = mirrored(80, 24, session);
        let mut old = OldEngine::new(80, 24);
        hub.publish(session, scrolling_output(10));
        old.feed(&scrolling_output(10));
        assert_same_classification(&hub.visible_screen_text(session).unwrap(), &old.screen());

        hub.record_resize(session, 120, 40);
        old.resize(120, 40);
        hub.publish(session, PERMISSION_SCREEN.to_vec());
        old.feed(PERMISSION_SCREEN);
        let screen = hub.visible_screen_text(session).unwrap();
        assert_eq!((screen.cols, screen.rows), (120, 40));
        assert_same_classification(&screen, &old.screen());

        hub.record_resize(session, 50, 12);
        old.resize(50, 12);
        let screen = hub.visible_screen_text(session).unwrap();
        assert_eq!((screen.cols, screen.rows), (50, 12));
        assert_same_classification(&screen, &old.screen());
    }

    /// `publish_gap` writes a seam line into the mirror that the old engine never received; the
    /// observer's text leaves it out (and restores the rows it scrolled away), so the reading is
    /// unchanged. A narrow pane wraps the seam over two rows.
    #[test]
    fn the_gap_seam_is_not_part_of_the_classified_text() {
        for (cols, rows) in [(80, 24), (40, 10)] {
            let session = format!("observer-gap-{cols}");
            let hub = mirrored(cols, rows, &session);
            let mut old = OldEngine::new(cols, rows);
            hub.publish(&session, scrolling_output(30));
            old.feed(&scrolling_output(30));
            hub.publish_gap(&session).unwrap();
            let screen = hub.visible_screen_text(&session).unwrap();
            assert!(!screen.text.contains("Ferryx: output stream gap"), "{:?}", screen.text);
            assert_same_classification(&screen, &old.screen());

            hub.publish(&session, b"after the gap\r\n".to_vec());
            old.feed(b"after the gap\r\n");
            hub.publish_gap(&session).unwrap();
            hub.publish(&session, PERMISSION_SCREEN.to_vec());
            old.feed(PERMISSION_SCREEN);
            let screen = hub.visible_screen_text(&session).unwrap();
            assert!(!screen.text.contains("Ferryx: output stream gap"), "{:?}", screen.text);
            assert_same_classification(&screen, &old.screen());
        }
    }

    /// Measures (records, does not assert) how long `visible_screen_text` holds the mirror
    /// lock for a full 300x100 screen with full scrollback: the copy-out in `visible_rows`.
    #[test]
    fn mirror_lock_hold_for_a_large_screen_is_recorded() {
        let mut mirror = crate::terminal::screen_mirror::ScreenMirror::new(300, 100).unwrap();
        let mut bytes = Vec::new();
        for line in 0..2_500 {
            bytes.extend_from_slice(format!("line {line:05} {}\r\n", "y".repeat(280)).as_bytes());
        }
        mirror.feed(&bytes);
        let mut holds = Vec::new();
        let mut rows = None;
        for _ in 0..20 {
            let started = Instant::now();
            rows = mirror.visible_rows();
            holds.push(started.elapsed());
        }
        holds.sort();
        eprintln!(
            "agent observer mirror lock hold 300x100: median={:?} max={:?}",
            holds[holds.len() / 2],
            holds[holds.len() - 1]
        );
        let screen = rows.unwrap().into_screen();
        assert_eq!((screen.cols, screen.rows), (300, 100));
        assert!(screen.text.contains("line 02499"));
    }
}
