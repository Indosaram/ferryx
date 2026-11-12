//! Telling a parent pane when the child it spawned blocks or stalls.
//!
//! A pane spawned with `agents spawn --parent` is watched from the moment its lineage is known.
//! Two things are worth the parent's attention: the child entered a demand the owner has not
//! acknowledged (it is in `needsYou`), and the child has reported `working` for longer than the
//! `stallRestart` threshold without its screen moving.
//!
//! Three rules keep the parent's pane readable:
//!
//! * one notice per episode, so a child that stays blocked is announced once and never again
//!   until it opens a new episode ([`ParentWatcher::observe_child`]);
//! * a notice never interrupts a running turn: while the parent is `working` the notice waits,
//!   and it goes out on the tick the parent reads idle
//!   ([`ParentWatcher::observe_parent`]);
//! * a parent that is gone takes its pending notices with it, so nothing is retried forever.
//!
//! The watcher holds no clock of its own: every call is given `now_ms`, so a test drives it with
//! a virtual clock and the daemon passes the bus clock.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use super::{AgentControlState, AgentStateSource};

/// The tag every line a parent receives from the daemon carries.
pub const PARENT_NOTICE_TAG: &str = "[ferryx]";

/// How long after a block a notice may still be waiting on a busy parent. A parent that is idle
/// gets its notice on the same tick, so this only bounds a parent that never comes back to idle.
pub const PARENT_NOTICE_DEADLINE_MS: i64 = 60_000;

/// What the watcher knows about a watched child that the bus cannot read for itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildContext {
    /// The pane that spawned this one.
    pub parent: String,
    /// The name the spawner gave the child, if any.
    pub name: Option<String>,
    /// Why the child stopped, in one line, when the daemon has one to hand.
    pub reason: Option<String>,
}

impl ChildContext {
    /// The name a notice calls the child.
    fn notice_name(&self) -> &str {
        self.name.as_deref().filter(|name| !name.trim().is_empty()).unwrap_or("a child pane")
    }
}

/// One reading of a watched child, reduced to what the watcher judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildReading {
    pub session_id: String,
    pub state: AgentControlState,
    pub state_source: AgentStateSource,
    pub prompt_id: Option<String>,
    pub screen_revision: Option<String>,
    pub state_since_ms: Option<i64>,
    pub screen_changed_at_ms: Option<i64>,
    /// Whether the owner already acknowledged the child's current episode. An acknowledged
    /// demand is `seen`, not `needsYou`, so it is never announced.
    pub read: bool,
}

/// Whether the watched child's parent can take a line right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentPresence {
    /// Mid-turn: a notice waits for the next idle.
    Working,
    /// Waiting for input or finished: a notice goes out now.
    Idle,
    /// Not on this daemon any more: every notice for it is dropped.
    Gone,
}

/// One line waiting to be typed into a parent pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParentNotice {
    pub parent_session_id: String,
    pub child_session_id: String,
    /// The episode that produced the notice, so a repeat of it is recognised.
    pub episode: String,
    pub text: String,
}

/// The `stallRestart` threshold the watcher uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentWatchConfig {
    pub stall_after_ms: i64,
}

impl Default for ParentWatchConfig {
    fn default() -> Self {
        let secs = ferryx_hub::supervisor::health::DEFAULT_STALL_SECS;
        Self { stall_after_ms: secs.saturating_mul(1000) }
    }
}

/// One watched child: which parent it belongs to, what was announced already, and what is still
/// waiting for that parent to be free.
#[derive(Debug, Default)]
struct ChildWatch {
    parent: String,
    name: Option<String>,
    reason: Option<String>,
    /// Every episode already announced or queued. A repeat of one is silent.
    announced: HashSet<String>,
    /// Notices due now, oldest first, held until the parent reads idle.
    pending: VecDeque<ParentNotice>,
    /// When the oldest pending notice was queued, for [`PARENT_NOTICE_DEADLINE_MS`].
    queued_at_ms: Option<i64>,
}

/// The daemon's per-child memory of what a spawn parent has already been told.
///
/// Not a task: the watcher never sleeps and never polls. Each observation tick feeds it one
/// child reading and the state of that child's parent, and it returns the notices to type.
#[derive(Debug)]
pub struct ParentWatcher {
    config: ParentWatchConfig,
    children: HashMap<String, ChildWatch>,
    /// Parents seen gone. A pane that went away is never watched again on this daemon: the
    /// observing ticks keep reporting its children, and re-creating a watch would turn a closed
    /// parent into a delivery loop.
    gone_parents: HashSet<String>,
}

impl ParentWatcher {
    pub fn new(config: ParentWatchConfig) -> Self {
        Self { config, children: HashMap::new(), gone_parents: HashSet::new() }
    }

    pub fn config(&self) -> ParentWatchConfig {
        self.config
    }

    /// How many children are being watched. A parent that went away leaves none behind.
    pub fn watched_children(&self) -> usize {
        self.children.len()
    }

    /// Drop everything remembered about `session_id`, including a notice still waiting.
    pub fn forget_child(&mut self, session_id: &str) {
        self.children.remove(session_id);
    }

    /// Judge one child reading and queue whatever notice it opens.
    ///
    /// A reading of a state that is neither an unacknowledged demand nor a stalled provider turn
    /// queues nothing and keeps the child watched, so a later block is still announced.
    pub fn observe_child(&mut self, context: &ChildContext, reading: &ChildReading, now_ms: i64) {
        if context.parent == reading.session_id {
            return;
        }
        // A parent that is gone takes its children with it, permanently.
        if self.gone_parents.contains(&context.parent) {
            return;
        }
        // Both episodes are judged before the child's entry is borrowed mutably, so the stall
        // threshold can be read from `self` while the entry is updated below.
        let stall = self.stall_episode(reading, now_ms);
        let mut due: Vec<(String, String)> = Vec::new();
        if let Some(episode) = demand_episode(reading) {
            let reason = context
                .reason
                .as_deref()
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or("waiting for your answer");
            due.push((
                episode,
                format!("{PARENT_NOTICE_TAG} {} needs input: {reason}", context.notice_name()),
            ));
        }
        if let Some(episode) = stall {
            let secs = self.config.stall_after_ms / 1000;
            due.push((
                episode,
                format!(
                    "{PARENT_NOTICE_TAG} {} looks stalled: no screen change for {secs}s",
                    context.notice_name()
                ),
            ));
        }

        let watch = self
            .children
            .entry(reading.session_id.clone())
            .or_insert_with(|| ChildWatch { parent: context.parent.clone(), ..ChildWatch::default() });
        watch.parent = context.parent.clone();
        watch.name = context.name.clone();
        watch.reason = context.reason.clone();
        for (episode, text) in due {
            // One notice per episode: a repeat of an announced one is silent.
            if !watch.announced.insert(episode.clone()) {
                continue;
            }
            watch.pending.push_back(ParentNotice {
                parent_session_id: watch.parent.clone(),
                child_session_id: reading.session_id.clone(),
                episode,
                text,
            });
            watch.queued_at_ms.get_or_insert(now_ms);
        }
    }

    /// The episode of a stalled turn: a provider-reported `working` whose screen has not moved
    /// for the threshold. A screen-sourced `working` is never stalled, because the observer
    /// already turns a quiet screen idle.
    fn stall_episode(&self, reading: &ChildReading, now_ms: i64) -> Option<String> {
        if reading.state != AgentControlState::Working
            || reading.state_source != AgentStateSource::Provider
        {
            return None;
        }
        let changed_at = reading.screen_changed_at_ms?;
        if now_ms.saturating_sub(changed_at) < self.config.stall_after_ms {
            return None;
        }
        let key = reading
            .screen_revision
            .clone()
            .or_else(|| reading.screen_changed_at_ms.map(|ms| ms.to_string()))
            .or_else(|| reading.state_since_ms.map(|ms| ms.to_string()))?;
        Some(format!("stall:{key}"))
    }

    /// Feed the state of one parent and take the notices it can receive now.
    ///
    /// `Idle` releases everything queued, `Working` releases nothing, and `Gone` drops the queue
    /// and every child of that parent so no later tick can deliver or retry.
    pub fn observe_parent(
        &mut self,
        parent_session_id: &str,
        presence: ParentPresence,
        now_ms: i64,
    ) -> Vec<ParentNotice> {
        match presence {
            ParentPresence::Gone => {
                self.gone_parents.insert(parent_session_id.to_string());
                self.children.retain(|_, watch| watch.parent != parent_session_id);
                return Vec::new();
            }
            ParentPresence::Working => return Vec::new(),
            ParentPresence::Idle => {}
        }
        let mut due = Vec::new();
        for (session_id, watch) in self.children.iter_mut() {
            if watch.parent != parent_session_id {
                continue;
            }
            // A parent that only comes back to idle long after the block still hears about it
            // once; the deadline bounds how long that is worth saying.
            let stale = watch
                .queued_at_ms
                .is_some_and(|queued| now_ms.saturating_sub(queued) > PARENT_NOTICE_DEADLINE_MS);
            if stale {
                watch.pending.clear();
            }
            while let Some(notice) = watch.pending.pop_front() {
                due.push(ParentNotice { child_session_id: session_id.clone(), ..notice });
            }
            watch.queued_at_ms = None;
        }
        due
    }
}

/// The episode a reading opens for `needsYou`: an unread demand. An open prompt is its own
/// episode under its prompt id, so a second prompt in the same pane is announced again; a block
/// the observer saw without an id falls back to when the state began.
fn demand_episode(reading: &ChildReading) -> Option<String> {
    if reading.read || reading.state != AgentControlState::Blocked {
        return None;
    }
    reading
        .prompt_id
        .clone()
        .map(|id| format!("prompt:{id}"))
        .or_else(|| reading.state_since_ms.map(|ms| format!("blocked:{ms}")))
}

/// Where a watched child's lineage and reason come from, and whether its parent is still here.
///
/// The bus knows a pane's state; only the daemon knows who spawned it, why it stopped and
/// whether the parent pane still exists. Both are one cheap synchronous read, which is why this
/// runs on the observer tick rather than in a task of its own.
pub trait WatchSource: Send + Sync {
    /// The spawn parent, the name the spawner gave the child and the child's reason line. `None`
    /// for a pane nobody spawned, which is never watched.
    fn facts(&self, session_id: &str) -> Option<ChildFacts>;
    /// Whether the parent pane can take a line, is mid-turn, or is gone.
    fn presence(&self, parent_session_id: &str) -> ParentPresence;
}

/// What a [`WatchSource`] knows about one child.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildFacts {
    pub parent: String,
    pub name: Option<String>,
    pub reason: Option<String>,
}

impl ChildFacts {
    fn context(&self) -> ChildContext {
        ChildContext { parent: self.parent.clone(), name: self.name.clone(), reason: self.reason.clone() }
    }
}

/// The daemon's tick-time driver: read the bus, judge every watched child, release what each
/// parent can take now.
///
/// Delivers nothing itself. [`ParentWatchDriver::tick`] returns the lines to type, so the caller
/// owns the send and its authorization, and the watcher stays free of IO.
pub struct ParentWatchDriver {
    watcher: ParentWatcher,
    source: Arc<dyn WatchSource>,
}

impl ParentWatchDriver {
    pub fn new(config: ParentWatchConfig, source: Arc<dyn WatchSource>) -> Self {
        Self { watcher: ParentWatcher::new(config), source }
    }

    pub fn watcher(&self) -> &ParentWatcher {
        &self.watcher
    }

    /// One pass over the bus at `now_ms`: observe every spawned pane, then release the notices
    /// its parent can receive. Children are seen before parents, so a block observed on this
    /// tick is delivered on this same tick when the parent already reads idle.
    pub fn tick(&mut self, bus: &super::events::AgentEventBus, now_ms: i64) -> Vec<ParentNotice> {
        let readings = bus.snapshot();
        let mut parents: Vec<String> = Vec::new();
        for observation in &readings {
            let Some(facts) = self.source.facts(&observation.session_id) else {
                continue;
            };
            let context = facts.context();
            if !parents.contains(&context.parent) {
                parents.push(context.parent.clone());
            }
            self.watcher.observe_child(
                &context,
                &ChildReading {
                    session_id: observation.session_id.clone(),
                    state: observation.state,
                    state_source: observation.state_source,
                    prompt_id: observation.prompt_id.clone(),
                    screen_revision: observation.screen_revision.clone(),
                    state_since_ms: observation.state_since_ms,
                    screen_changed_at_ms: observation.screen_changed_at_ms,
                    read: observation.read,
                },
                now_ms,
            );
        }
        let mut due = Vec::new();
        for parent in parents {
            let presence = self.source.presence(&parent);
            due.extend(self.watcher.observe_parent(&parent, presence, now_ms));
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock the test moves by hand: no test here sleeps or reads the wall clock.
    struct VirtualClock {
        now_ms: i64,
    }

    impl VirtualClock {
        fn new() -> Self {
            Self { now_ms: 1_000_000 }
        }

        fn advance_ms(&mut self, ms: i64) -> i64 {
            self.now_ms += ms;
            self.now_ms
        }
    }

    fn child_reading(state: AgentControlState, source: AgentStateSource) -> ChildReading {
        ChildReading {
            session_id: "child-1".into(),
            state,
            state_source: source,
            prompt_id: None,
            screen_revision: Some("rev-1".into()),
            state_since_ms: Some(1_000_000),
            screen_changed_at_ms: Some(1_000_000),
            read: false,
        }
    }

    fn context() -> ChildContext {
        ChildContext {
            parent: "parent-1".into(),
            name: Some("worker".into()),
            reason: Some("which branch?".into()),
        }
    }

    fn watcher() -> ParentWatcher {
        ParentWatcher::new(ParentWatchConfig { stall_after_ms: 300_000 })
    }

    #[test]
    fn parent_watch_a_blocked_child_reaches_an_idle_parent_once_per_episode() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Provider);
        reading.prompt_id = Some("p1".into());

        watch.observe_child(&context(), &reading, clock.now_ms);
        let due = watch.observe_parent("parent-1", ParentPresence::Idle, clock.now_ms);
        assert_eq!(due.len(), 1, "an idle parent hears about the block on the same tick");
        assert_eq!(due[0].text, "[ferryx] worker needs input: which branch?");
        assert_eq!(due[0].child_session_id, "child-1");
        assert_eq!(due[0].parent_session_id, "parent-1");

        // Later ticks of the same episode are silent.
        for _ in 0..5 {
            let now = clock.advance_ms(5_000);
            watch.observe_child(&context(), &reading, now);
            assert!(watch.observe_parent("parent-1", ParentPresence::Idle, now).is_empty());
        }
    }

    #[test]
    fn parent_watch_a_second_prompt_in_the_same_pane_is_announced_again() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Provider);
        reading.prompt_id = Some("p1".into());
        watch.observe_child(&context(), &reading, clock.now_ms);
        assert_eq!(watch.observe_parent("parent-1", ParentPresence::Idle, clock.now_ms).len(), 1);

        let now = clock.advance_ms(10_000);
        reading.prompt_id = Some("p2".into());
        reading.state_since_ms = Some(now);
        watch.observe_child(&context(), &reading, now);
        let due = watch.observe_parent("parent-1", ParentPresence::Idle, now);
        assert_eq!(due.len(), 1, "a new prompt id is a new episode");
        assert_eq!(due[0].episode, "prompt:p2");
    }

    #[test]
    fn parent_watch_an_acknowledged_demand_is_never_announced() {
        let clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Provider);
        reading.prompt_id = Some("p1".into());
        reading.read = true;
        watch.observe_child(&context(), &reading, clock.now_ms);
        assert!(watch.observe_parent("parent-1", ParentPresence::Idle, clock.now_ms).is_empty());
    }

    #[test]
    fn parent_watch_a_blocked_child_waits_for_the_parent_to_go_idle() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Provider);
        reading.prompt_id = Some("p1".into());

        watch.observe_child(&context(), &reading, clock.now_ms);
        let working = clock.advance_ms(1_000);
        assert!(
            watch.observe_parent("parent-1", ParentPresence::Working, working).is_empty(),
            "a running parent is never interrupted"
        );
        // The block persists while the parent works.
        let still = clock.advance_ms(30_000);
        watch.observe_child(&context(), &reading, still);
        assert!(watch.observe_parent("parent-1", ParentPresence::Working, still).is_empty());

        let idle = clock.advance_ms(1_000);
        let due = watch.observe_parent("parent-1", ParentPresence::Idle, idle);
        assert_eq!(due.len(), 1, "the held notice goes out on the first idle tick");
        assert_eq!(due[0].text, "[ferryx] worker needs input: which branch?");
    }

    #[test]
    fn parent_watch_delivery_stays_within_sixty_seconds_of_the_block() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Provider);
        reading.prompt_id = Some("p1".into());
        let blocked_at = clock.now_ms;
        watch.observe_child(&context(), &reading, blocked_at);

        // The parent finishes its turn at the worst moment still worth telling.
        let idle_at = clock.advance_ms(PARENT_NOTICE_DEADLINE_MS - 1_000);
        let due = watch.observe_parent("parent-1", ParentPresence::Idle, idle_at);
        assert_eq!(due.len(), 1);
        assert!(
            idle_at - blocked_at <= PARENT_NOTICE_DEADLINE_MS,
            "a notice reaches an idle parent well inside a minute"
        );
    }

    #[test]
    fn parent_watch_a_stale_pending_notice_is_dropped_instead_of_delivered_late() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Provider);
        reading.prompt_id = Some("p1".into());
        watch.observe_child(&context(), &reading, clock.now_ms);

        let late = clock.advance_ms(PARENT_NOTICE_DEADLINE_MS + 1);
        assert!(
            watch.observe_parent("parent-1", ParentPresence::Idle, late).is_empty(),
            "a notice nobody could act on any more is not typed into the pane"
        );
    }

    #[test]
    fn parent_watch_a_closed_parent_receives_nothing_and_retries_nothing() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Provider);
        reading.prompt_id = Some("p1".into());
        watch.observe_child(&context(), &reading, clock.now_ms);
        assert_eq!(watch.watched_children(), 1);

        let gone = clock.advance_ms(1_000);
        assert!(watch.observe_parent("parent-1", ParentPresence::Gone, gone).is_empty());
        assert_eq!(watch.watched_children(), 0, "a gone parent leaves no child to watch");

        // The child keeps blocking; nothing is delivered and no loop starts.
        for _ in 0..3 {
            let now = clock.advance_ms(10_000);
            watch.observe_child(&context(), &reading, now);
            assert!(watch.observe_parent("parent-1", ParentPresence::Idle, now).is_empty());
        }
    }

    #[test]
    fn parent_watch_a_stalled_provider_child_notifies_once_past_the_threshold() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let reading = child_reading(AgentControlState::Working, AgentStateSource::Provider);

        // Inside the threshold: silence.
        watch.observe_child(&context(), &reading, clock.now_ms);
        assert!(watch.observe_parent("parent-1", ParentPresence::Idle, clock.now_ms).is_empty());

        let past = clock.advance_ms(300_000);
        watch.observe_child(&context(), &reading, past);
        let due = watch.observe_parent("parent-1", ParentPresence::Idle, past);
        assert_eq!(due.len(), 1, "one stall line once the screen has not moved for the threshold");
        assert_eq!(due[0].text, "[ferryx] worker looks stalled: no screen change for 300s");

        // The same unmoved screen never notifies twice.
        let later = clock.advance_ms(60_000);
        watch.observe_child(&context(), &reading, later);
        assert!(watch.observe_parent("parent-1", ParentPresence::Idle, later).is_empty());

        // A moved screen that stalls again is a new episode.
        let mut moved = reading.clone();
        moved.screen_revision = Some("rev-2".into());
        moved.screen_changed_at_ms = Some(later);
        watch.observe_child(&context(), &moved, later);
        let again = clock.advance_ms(300_000);
        watch.observe_child(&context(), &moved, again);
        assert_eq!(watch.observe_parent("parent-1", ParentPresence::Idle, again).len(), 1);
    }

    #[test]
    fn parent_watch_a_screen_sourced_working_child_is_never_stalled() {
        let mut clock = VirtualClock::new();
        let mut watch = watcher();
        let reading = child_reading(AgentControlState::Working, AgentStateSource::Screen);
        watch.observe_child(&context(), &reading, clock.now_ms);
        let past = clock.advance_ms(3_600_000);
        watch.observe_child(&context(), &reading, past);
        assert!(
            watch.observe_parent("parent-1", ParentPresence::Idle, past).is_empty(),
            "the observer already turns a quiet screen idle"
        );
    }

    #[test]
    fn parent_watch_the_notice_falls_back_when_no_name_or_reason_is_known() {
        let clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Screen);
        reading.prompt_id = Some("p1".into());
        let bare = ChildContext { parent: "parent-1".into(), name: None, reason: None };
        watch.observe_child(&bare, &reading, clock.now_ms);
        let due = watch.observe_parent("parent-1", ParentPresence::Idle, clock.now_ms);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].text, "[ferryx] a child pane needs input: waiting for your answer");
    }

    #[test]
    fn parent_watch_a_child_never_notifies_its_own_pane() {
        let clock = VirtualClock::new();
        let mut watch = watcher();
        let mut reading = child_reading(AgentControlState::Blocked, AgentStateSource::Screen);
        reading.prompt_id = Some("p1".into());
        let self_parent = ChildContext { parent: "child-1".into(), name: None, reason: None };
        watch.observe_child(&self_parent, &reading, clock.now_ms);
        assert_eq!(watch.watched_children(), 0);
        assert!(watch.observe_parent("child-1", ParentPresence::Idle, clock.now_ms).is_empty());
    }

    /// One fixed set of spawn facts, so the driver tests exercise the bus without a daemon.
    struct FixtureSource {
        parent: String,
        presence: ParentPresence,
    }

    impl WatchSource for FixtureSource {
        fn facts(&self, session_id: &str) -> Option<ChildFacts> {
            (session_id == "child-1").then(|| ChildFacts {
                parent: self.parent.clone(),
                name: Some("worker".into()),
                reason: Some("which branch?".into()),
            })
        }

        fn presence(&self, _parent_session_id: &str) -> ParentPresence {
            self.presence
        }
    }

    #[test]
    fn parent_watch_the_driver_delivers_a_block_from_the_bus_on_the_same_tick() {
        let mut clock = VirtualClock::new();
        let bus = super::super::events::AgentEventBus::new(8);
        bus.publish(super::super::events::ObservationDraft {
            session_id: "child-1".into(),
            state: AgentControlState::Blocked,
            state_source: AgentStateSource::Provider,
            tier: super::super::AgentTier::Native,
            prompt_id: Some("p1".into()),
            kind: None,
            screen_revision: Some("rev-1".into()),
            workspace_id: None,
        });
        let source = Arc::new(FixtureSource { parent: "parent-1".into(), presence: ParentPresence::Idle });
        let mut driver = ParentWatchDriver::new(ParentWatchConfig { stall_after_ms: 300_000 }, source);

        let due = driver.tick(&bus, clock.now_ms);
        assert_eq!(due.len(), 1, "an idle parent hears about the block on the same tick");
        assert_eq!(due[0].text, "[ferryx] worker needs input: which branch?");

        // A repeat tick of the same episode is silent.
        let later = clock.advance_ms(2_000);
        assert!(driver.tick(&bus, later).is_empty());
    }

    #[test]
    fn parent_watch_the_driver_holds_a_block_while_the_parent_works() {
        let clock = VirtualClock::new();
        let bus = super::super::events::AgentEventBus::new(8);
        bus.publish(super::super::events::ObservationDraft {
            session_id: "child-1".into(),
            state: AgentControlState::Blocked,
            state_source: AgentStateSource::Provider,
            tier: super::super::AgentTier::Native,
            prompt_id: Some("p1".into()),
            kind: None,
            screen_revision: Some("rev-1".into()),
            workspace_id: None,
        });
        let source = Arc::new(FixtureSource { parent: "parent-1".into(), presence: ParentPresence::Working });
        let mut driver = ParentWatchDriver::new(ParentWatchConfig { stall_after_ms: 300_000 }, source);
        assert!(driver.tick(&bus, clock.now_ms).is_empty(), "a busy parent is not interrupted");
    }

    #[test]
    fn parent_watch_the_driver_drops_everything_when_the_parent_is_gone() {
        let clock = VirtualClock::new();
        let bus = super::super::events::AgentEventBus::new(8);
        bus.publish(super::super::events::ObservationDraft {
            session_id: "child-1".into(),
            state: AgentControlState::Blocked,
            state_source: AgentStateSource::Provider,
            tier: super::super::AgentTier::Native,
            prompt_id: Some("p1".into()),
            kind: None,
            screen_revision: Some("rev-1".into()),
            workspace_id: None,
        });
        let source = Arc::new(FixtureSource { parent: "parent-1".into(), presence: ParentPresence::Gone });
        let mut driver = ParentWatchDriver::new(ParentWatchConfig { stall_after_ms: 300_000 }, source);
        assert!(driver.tick(&bus, clock.now_ms).is_empty());
        assert_eq!(driver.watcher().watched_children(), 0, "a gone parent leaves no watch behind");
    }
}
