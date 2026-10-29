//! Per-session agent observer.
//!
//! One task per live PTY wakes on output-hub output (debounced) and on agent-state
//! self-reports. It keeps its own headless terminal at the pane's real size, runs the
//! existing screen detectors, and publishes a slim reading when the reading changes.
//! Idle for a screen-only agent is a quiet-time timer, not a poll. The shared screen
//! mirror and its `event_tx` are never used.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::{broadcast, watch};

use crate::agent_control::events::{classify_screen, AgentEventBus};
use crate::agent_control::ProviderReport;
use crate::daemon::agent_state::AgentStateHub;
use crate::native_terminal::{NativeTerminal, TerminalEngine};
use crate::terminal::TerminalService;

const DEFAULT_CAP: usize = 64;

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

/// Observes every daemon-owned PTY the cap allows.
pub struct AgentObserverHub {
    terminals: Arc<TerminalService>,
    bus: Arc<AgentEventBus>,
    slots: Slots,
    cap: usize,
    timing: ObserverTiming,
    sources: Mutex<Option<Arc<AgentStateHub>>>,
    workspace_of: Mutex<Option<WorkspaceLookup>>,
    generation: AtomicU64,
}

impl AgentObserverHub {
    pub fn new(terminals: Arc<TerminalService>, bus: Arc<AgentEventBus>) -> Self {
        Self::with_limits(terminals, bus, default_cap(), ObserverTiming::default())
    }

    pub fn with_limits(
        terminals: Arc<TerminalService>,
        bus: Arc<AgentEventBus>,
        cap: usize,
        timing: ObserverTiming,
    ) -> Self {
        Self {
            terminals,
            bus,
            slots: Arc::new(Mutex::new(HashMap::new())),
            cap: cap.max(1),
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

    /// Start observing `session_id` unless it is already observed or the cap is full.
    pub fn watch(&self, session_id: &str) -> bool {
        let mut slots = self.slots.lock();
        if slots.contains_key(session_id) {
            return true;
        }
        if slots.len() >= self.cap {
            return false;
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

    /// Observe every PTY the terminal service currently owns, up to the cap.
    pub fn watch_live(&self) {
        for session_id in self.terminals.list_sessions() {
            if !self.watch(&session_id) {
                break;
            }
        }
    }
}

fn default_cap() -> usize {
    std::env::var("FERRYX_AGENT_OBSERVER_CAP")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_CAP)
}

fn pane_size(terminals: &TerminalService, session_id: &str) -> (u16, u16) {
    let (cols, rows) = terminals
        .get_session(session_id)
        .map(|pty| pty.get_size())
        .unwrap_or((80, 24));
    let cols = if cols == 0 { 80 } else { cols };
    let rows = if rows == 0 { 24 } else { rows };
    (cols, rows)
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

fn screen_text(engine: &NativeTerminal) -> String {
    let rows = engine.rows().unwrap_or(0) as usize;
    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        match engine.screen_row_text(row) {
            Ok(text) => lines.push(text.trim_end().to_string()),
            Err(_) => break,
        }
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

fn publish_if_changed(
    bus: &AgentEventBus,
    session_id: &str,
    workspace_of: Option<&WorkspaceLookup>,
    provider: Option<&ProviderReport>,
    engine: &NativeTerminal,
    quiet_elapsed: bool,
) {
    let (cols, rows) = engine.dimensions().unwrap_or((80, 24));
    let mut draft = classify_screen(provider, cols, rows, &screen_text(engine), quiet_elapsed);
    draft.session_id = session_id.to_string();
    // The observer sees every screen change, so it is what notices a card leave and come back:
    // an identical re-ask after an answer is a new occurrence id, and the push fires for it.
    draft.prompt_id = crate::remote::reference_chat::prompts::note_reference_prompt_occurrence(
        session_id,
        draft.prompt_id.as_deref(),
        draft.screen_revision.as_deref().unwrap_or_default(),
    );
    draft.workspace_id = workspace_of.and_then(|lookup| lookup(session_id));
    let changed = match bus.latest(session_id) {
        Some(previous) => !previous.same_reading(&draft),
        None => true,
    };
    if changed {
        bus.publish(draft);
    }
}

fn rebuild(engine: &mut NativeTerminal, history: &[u8]) -> bool {
    engine.reset();
    engine.feed(history).is_ok()
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
    let Some((history, mut output)) = service.output_hub().subscribe(&session_id) else {
        return;
    };
    let (cols, rows) = pane_size(&service, &session_id);
    drop(service);
    let mut engine = match NativeTerminal::new(cols, rows) {
        Ok(engine) => engine,
        Err(error) => {
            tracing::warn!(%error, %session_id, "agent observer could not open a headless terminal");
            return;
        }
    };
    engine.set_pty_writes_suppressed(true);
    if engine.set_scrollback_limit_lines(Some(0)).is_err() || engine.feed(&history).is_err() {
        tracing::warn!(%session_id, "agent observer could not replay the pane");
        return;
    }
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
                    Ok(bytes) => {
                        if engine.feed(&bytes).is_err() {
                            tracing::warn!(%session_id, "agent observer stopped after a feed error");
                            break;
                        }
                        dirty = true;
                        quiet_done = false;
                        debounce_at = tokio::time::Instant::now() + timing.debounce;
                        quiet_at = tokio::time::Instant::now() + timing.quiet;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        let Some((history, fresh)) = terminals
                            .upgrade()
                            .and_then(|service| service.output_hub().subscribe(&session_id))
                        else {
                            break;
                        };
                        output = fresh;
                        if !rebuild(&mut engine, &history) {
                            break;
                        }
                        dirty = true;
                        quiet_done = false;
                        debounce_at = tokio::time::Instant::now() + timing.debounce;
                        quiet_at = tokio::time::Instant::now() + timing.quiet;
                    }
                }
            }
            update = agent_wait => {
                match update {
                    Ok(update) if update.state.session_id == session_id => {
                        dirty = true;
                        quiet_done = false;
                        debounce_at = tokio::time::Instant::now();
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
                let Some(service) = terminals.upgrade() else { break };
                sync_size(&service, &session_id, &mut engine);
                drop(service);
                let provider = sources.as_ref().and_then(|states| provider_report(states, &session_id));
                publish_if_changed(&bus, &session_id, workspace_of.as_ref(), provider.as_ref(), &engine, false);
                dirty = false;
            }
            _ = tokio::time::sleep_until(quiet_at), if !dirty && !quiet_done => {
                let Some(service) = terminals.upgrade() else { break };
                sync_size(&service, &session_id, &mut engine);
                drop(service);
                let provider = sources.as_ref().and_then(|states| provider_report(states, &session_id));
                publish_if_changed(&bus, &session_id, workspace_of.as_ref(), provider.as_ref(), &engine, true);
                quiet_done = true;
            }
        }
    }
}

fn sync_size(terminals: &TerminalService, session_id: &str, engine: &mut NativeTerminal) {
    let (cols, rows) = pane_size(terminals, session_id);
    if engine.cols().ok() == Some(cols) && engine.rows().ok() == Some(rows) {
        return;
    }
    if let Err(error) = engine.resize(cols, rows, 8, 16) {
        tracing::debug!(%error, %session_id, "agent observer kept the previous pane size");
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

    fn hub(
        cap: usize,
    ) -> (
        Arc<TerminalService>,
        Arc<AgentEventBus>,
        Arc<AgentObserverHub>,
    ) {
        let terminals = Arc::new(TerminalService::default());
        let bus = Arc::new(AgentEventBus::new(32));
        let observers = Arc::new(AgentObserverHub::with_limits(
            Arc::clone(&terminals),
            Arc::clone(&bus),
            cap,
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
        let (terminals, bus, observers) = hub(4);
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
    async fn the_cap_refuses_a_second_live_session() {
        let (terminals, _bus, observers) = hub(1);
        terminals.output_hub().register_session("observer-cap-a");
        terminals.output_hub().register_session("observer-cap-b");
        assert!(observers.watch("observer-cap-a"));
        assert!(!observers.watch("observer-cap-b"));
        assert_eq!(observers.watched(), 1);
        observers.unwatch("observer-cap-a");
    }

    #[tokio::test]
    async fn a_provider_report_is_published_without_waiting_for_the_quiet_timer() {
        let (terminals, bus, observers) = hub(4);
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
}
