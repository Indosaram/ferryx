//! Production wiring of the supervisor's autonomy adapters in the daemon (plan todos 5-8):
//! the real `SessionControl` / `FleetSource` behind the engine's session tools, the panel's
//! sources (action log, rules, escalations), the owner channel publishers, the answer history's
//! callers, and the pause flag. Everything here runs only for the owner's own processes: the
//! agent-control owner socket, the owner-authorized HTTP routes and the daemon's observer.
use std::sync::Arc;

use async_trait::async_trait;
use ferryx_hub::goals::loop_driver::{on_turn_settled_with, QuotaRecovery};
use ferryx_hub::supervisor::adapters::{HostDirectory, PairedHost, RemoteFleetSource, SupervisorAgentControl};
use ferryx_hub::supervisor::autonomy::{Autonomy, PanelPublisher};
use ferryx_hub::supervisor::health::SessionHealthClassifier;
use ferryx_hub::supervisor::session_control::AgentSessionControl;
use ferryx_hub::supervisor::panel_contract::{
    ActionEntry, Escalation, FleetSession, SessionControl, SessionRef, StandingRule,
};
use ferryx_hub::supervisor::recovery::{
    CardError, Clock, LoopRetry, PauseGate, RecoveryDeps, RecoveryEscalations, RecoveryScheduler,
    RetryOutcome, SystemClock, CARD_PREFIX,
};

use super::HubLaunch;
use crate::agent_control::AgentControlOp;
use crate::remote::state::RemoteGatewayState;

/// The one autonomy service of this daemon, installed when the hub opens.
static AUTONOMY: std::sync::OnceLock<Arc<Autonomy>> = std::sync::OnceLock::new();

/// The engine's session tools, built once with the real adapters.
static SESSION_TOOLS: std::sync::OnceLock<Arc<ferryx_hub::meta::mhc_supervisor::SessionTools>> =
    std::sync::OnceLock::new();

/// The session tools to hand the engine's work handler (`SlackWorkTools::with_sessions`).
pub fn session_tools() -> Option<Arc<ferryx_hub::meta::mhc_supervisor::SessionTools>> {
    SESSION_TOOLS.get().cloned()
}

/// The installed autonomy service, if the hub is open in this daemon.
pub fn installed() -> Option<Arc<Autonomy>> {
    AUTONOMY.get().cloned()
}

/// The one recovery scheduler of this daemon, installed with the autonomy service.
static RECOVERY: std::sync::OnceLock<Arc<RecoveryScheduler>> = std::sync::OnceLock::new();

/// The installed recovery scheduler, if the hub is open in this daemon.
pub fn recovery() -> Option<Arc<RecoveryScheduler>> {
    RECOVERY.get().cloned()
}

/// Builds the recovery scheduler over the autonomy service's own stores, pause gate and
/// escalation registry. `control`, `fleet` and `clock` are the seams: production passes the
/// supervisor-marked session control, the full fleet and the system clock.
pub fn build_recovery(
    autonomy: &Arc<Autonomy>,
    control: Arc<dyn SessionControl>,
    fleet: Arc<dyn ferryx_hub::meta::mhc_supervisor::FleetSource>,
    clock: Arc<dyn Clock>,
) -> Arc<RecoveryScheduler> {
    let pause: Arc<dyn PauseGate> = autonomy.gate();
    let service: Arc<Autonomy> = Arc::clone(autonomy);
    let escalations: Arc<dyn RecoveryEscalations> = service;
    Arc::new(RecoveryScheduler::new(RecoveryDeps {
        fleet,
        control,
        rules: autonomy.rule_store(),
        log: autonomy.action_log(),
        classifier: Arc::new(SessionHealthClassifier),
        clock,
        pause,
        escalations,
    }))
}

/// An owner's tap on a recovery card (`recovery:<incident>`), carried out by the recovery
/// scheduler. `None` when `prompt_id` is not a recovery card, so the caller answers it as an
/// ordinary pane prompt. A recovery card has no pane prompt behind it: nothing is typed.
pub async fn answer_recovery_card(prompt_id: &str, option_index: Option<u32>) -> Option<Result<(), CardError>> {
    if !prompt_id.starts_with(CARD_PREFIX) {
        return None;
    }
    let Some(scheduler) = recovery() else {
        return Some(Err(CardError::NotOpen));
    };
    let Some(option) = option_index.and_then(|index| usize::try_from(index).ok()) else {
        return Some(Err(CardError::BadOption));
    };
    Some(scheduler.answer_card(prompt_id, option).await)
}

/// Hands a goal lane's quota fault (the supervisor's own model is out of quota) to recovery,
/// which schedules the retry instead of the loop stopping the lane.
struct GoalQuota {
    scheduler: Arc<RecoveryScheduler>,
}

#[async_trait]
impl QuotaRecovery for GoalQuota {
    async fn quota_fault(&self, lane_key: &str, session_id: &str, detail: &str) -> bool {
        let session_ref = SessionRef::new(
            &crate::remote::reference_chat::files::reference_host_id(),
            session_id,
        );
        self.scheduler
            .report_goal_quota(lane_key, Some(session_ref), detail)
            .await;
        true
    }
}

/// The goal loop's quota recovery, when the recovery scheduler is installed.
pub fn quota_recovery() -> Option<Arc<dyn QuotaRecovery>> {
    let scheduler = recovery()?;
    let quota: Arc<dyn QuotaRecovery> = Arc::new(GoalQuota { scheduler });
    Some(quota)
}

/// Re-runs the goal loop for a lane whose quota fault recovery waited out.
pub(super) struct GoalRetry {
    pool: ferryx_hub::goals::SqlitePool,
    bridge: Arc<super::DaemonBridge>,
    quota: Arc<dyn QuotaRecovery>,
}

impl GoalRetry {
    pub(super) fn new(
        pool: ferryx_hub::goals::SqlitePool,
        bridge: Arc<super::DaemonBridge>,
        quota: Arc<dyn QuotaRecovery>,
    ) -> Self {
        Self { pool, bridge, quota }
    }
}

#[async_trait]
impl LoopRetry for GoalRetry {
    async fn retry(&self, lane_key: &str) -> ferryx_hub::error::Result<()> {
        self.retry_outcome(lane_key).await.map(|_| ())
    }

    /// `Ok(None)` from the goal loop means the lane has no goal to run: nothing was resumed.
    async fn retry_outcome(&self, lane_key: &str) -> ferryx_hub::error::Result<RetryOutcome> {
        let judge = ferryx_hub::judge::Judge::from_config(ferryx_hub::judge::typesafe_from_env());
        let outcome = on_turn_settled_with(
            &self.pool,
            &judge,
            self.bridge.as_ref(),
            lane_key,
            true,
            Some(self.quota.as_ref()),
        )
        .await
        .map_err(|error| ferryx_hub::error::HubError::message(error.to_string()))?;
        Ok(retry_outcome_of(outcome.as_ref()))
    }

    /// The lane's bounded retries ran out: the loop's own stop path blocks it as degraded.
    async fn give_up(&self, lane_key: &str, detail: &str) -> ferryx_hub::error::Result<()> {
        ferryx_hub::goals::loop_driver::stop_degraded(&self.pool, lane_key, detail)
            .await
            .map(|_| ())
            .map_err(|error| ferryx_hub::error::HubError::message(error.to_string()))
    }
}

/// What a goal-loop pass means for recovery: only a pass that left the lane running is a
/// resume. `None` is a lane with no goal; a pass that stopped the lane (degraded supervisor,
/// unavailable judge, any stop rule) resumed nothing.
fn retry_outcome_of(outcome: Option<&ferryx_hub::goals::loop_driver::LoopOutcome>) -> RetryOutcome {
    match outcome {
        None => RetryOutcome::NothingToResume,
        Some(outcome) => match outcome.stop_reason {
            Some(reason) => RetryOutcome::Stopped(reason.to_string()),
            None => RetryOutcome::Resumed,
        },
    }
}

/// Publishes through the daemon's owner event channel (`panel.decision|action|rules`).
pub struct OwnerChannel;

impl PanelPublisher for OwnerChannel {
    fn decision(&self, session_ref: &SessionRef, decision: Option<&Escalation>) {
        super::publish_panel_decision(session_ref, decision);
    }

    fn action(&self, action: &ActionEntry) {
        super::publish_panel_action(action);
    }

    fn rules(&self, rules: &[StandingRule]) {
        super::publish_panel_rules(rules);
    }
}

/// The paired computers with a machine grant, from the daemon's own paired-host service.
pub struct PairedDirectory {
    client: crate::daemon::client::DaemonClient,
}

#[async_trait]
impl HostDirectory for PairedDirectory {
    async fn hosts(&self) -> Vec<PairedHost> {
        use crate::paired_host::inventory::{AuthStatus, GrantScope};
        let Ok(hosts) = self.client.paired_host_list().await else {
            return Vec::new();
        };
        hosts
            .into_iter()
            .filter(|host| {
                host.grant_scope == GrantScope::Machine && host.auth_status == AuthStatus::Paired
            })
            .map(|host| PairedHost {
                online: host.online && host.unavailable_reason.is_none(),
                label: host.display_label,
                host_id: host.host_id,
            })
            .collect()
    }
}

/// Host ids that mean this computer in a session ref.
fn local_host_ids() -> Vec<String> {
    vec![
        ferryx_hub::agent_control::LOCAL_HOST_ID.to_owned(),
        crate::remote::reference_chat::files::reference_host_id(),
    ]
}

/// This computer's sessions and every paired computer's, merged with the stale rule. The local
/// list is read in-process; the paired hosts go through the hub's agent control. Non-owner
/// redaction is unchanged: callers of this are owner-authorized (the panel and fleet routes,
/// the owner socket); a non-owner never reaches it.
pub async fn fleet_sessions(state: &Arc<RemoteGatewayState>) -> Vec<FleetSession> {
    let mut sessions = super::local_fleet_sessions(state).await;
    if let Some(source) = REMOTE_FLEET.get() {
        match ferryx_hub::meta::mhc_supervisor::FleetSource::list(source.as_ref()).await {
            Ok(remote) => sessions.extend(remote),
            Err(error) => tracing::warn!(%error, "the paired-host fleet was not read"),
        }
    }
    sessions
}

static REMOTE_FLEET: std::sync::OnceLock<Arc<RemoteFleetSource>> = std::sync::OnceLock::new();

/// The engine's `FleetSource`: this computer plus the paired hosts.
struct FullFleet {
    state: Arc<RemoteGatewayState>,
}

#[async_trait]
impl ferryx_hub::meta::mhc_supervisor::FleetSource for FullFleet {
    async fn list(&self) -> ferryx_hub::error::Result<Vec<FleetSession>> {
        Ok(fleet_sessions(&self.state).await)
    }
}

/// Opens the stores, installs the panel sources, the rule store, the pause flag and builds the
/// engine's session tools. Called once per daemon, when the hub opens; a second call keeps the first.
pub(super) async fn install(launch: &HubLaunch) {
    if AUTONOMY.get().is_some() {
        return;
    }
    let state = Arc::clone(launch.server.remote_state());
    let pool = launch.hub.pool.clone();
    let bridge: Arc<dyn ferryx_hub::agent_control::AgentControl> = launch.bridge.clone();
    // One owner-guarded, host-routed, supervisor-marked agent control under every session tool
    // and every autonomous answer (T6's `AgentSessionControl` runs on top of it).
    let control: Arc<dyn ferryx_hub::agent_control::AgentControl> =
        Arc::new(SupervisorAgentControl::new(bridge, local_host_ids(), pool.clone()));
    let session_control: Arc<dyn SessionControl> = Arc::new(AgentSessionControl::new(control.clone()));
    let autonomy = match Autonomy::open(pool.clone(), session_control.clone(), Arc::new(OwnerChannel)).await {
        Ok(autonomy) => autonomy,
        Err(error) => {
            tracing::warn!(%error, "supervisor autonomy was not opened; the panel stays empty");
            return;
        }
    };
    // Reading the paired hosts needs no owner identity (a fresh install has none yet), so the
    // fleet reads the bridge directly; every write goes through the guarded control above.
    let remote = Arc::new(RemoteFleetSource::new(
        launch.bridge.clone(),
        pool.clone(),
        Arc::new(PairedDirectory { client: launch.bridge.client.clone() }),
    ));
    let _ = REMOTE_FLEET.set(remote);
    super::install_panel_sources(ferryx_hub::fleet::PanelSources {
        log: Some(autonomy.action_log()),
        rules: Some(autonomy.rule_store()),
        escalations: Some(autonomy.escalation_source()),
    });
    super::install_rule_store(autonomy.rule_store());
    super::set_panel_paused(ferryx_hub::control::is_paused(&pool).await.unwrap_or(false));
    let scheduler = build_recovery(
        &autonomy,
        session_control,
        Arc::new(FullFleet { state: Arc::clone(&state) }),
        Arc::new(SystemClock),
    );
    let _ = RECOVERY.set(scheduler);
    let tools = Arc::new(
        ferryx_hub::meta::mhc_supervisor::SessionTools::new(
            Arc::new(AgentSessionControl::new(control)),
            Arc::new(FullFleet { state }),
        )
        .with_gate(autonomy.gate()),
    );
    let _ = SESSION_TOOLS.set(tools);
    let _ = AUTONOMY.set(autonomy);
}

/// One pane observation: read the session in the fleet shape and let autonomy answer or
/// escalate its pending prompt. Event driven, called from the daemon's observer loop.
pub async fn observe(state: &Arc<RemoteGatewayState>, session_id: &str) {
    let Some(autonomy) = installed() else { return };
    match super::local_fleet_session(state, session_id).await {
        Some(session) => autonomy.observe(&session).await,
        None => {
            let session_ref = SessionRef::new(
                &crate::remote::reference_chat::files::reference_host_id(),
                session_id,
            );
            autonomy.clear(&session_ref);
        }
    }
}

/// An answer typed into a pane (card tap, phone, desktop or terminal) by anyone but the
/// supervisor: the owner's. Claimed before the write and recorded once it went through.
pub struct PendingOwnerAnswer {
    autonomy: Arc<Autonomy>,
    answer: ferryx_hub::supervisor::autonomy::OwnerAnswer,
    option_index: usize,
}

impl PendingOwnerAnswer {
    pub async fn recorded(self) {
        self.autonomy.record_owner_answer(self.answer, self.option_index).await;
    }
}

/// The owner answer behind an agent-control `Answer` op, when it is the owner's (not a
/// supervisor request id) and names a tracked prompt.
pub fn owner_answer_of(op: &AgentControlOp) -> Option<PendingOwnerAnswer> {
    owner_answer_with(&installed()?, op)
}

fn owner_answer_with(autonomy: &Arc<Autonomy>, op: &AgentControlOp) -> Option<PendingOwnerAnswer> {
    let AgentControlOp::Answer { session_id, prompt_id, answer, request_id, .. } = op else {
        return None;
    };
    if ferryx_hub::supervisor::adapters::is_supervisor_request(request_id.as_deref()) {
        return None;
    }
    let option_index = usize::try_from(answer.option_index?).ok()?;
    let claimed = autonomy.peek_owner_answer(session_id, prompt_id)?;
    Some(PendingOwnerAnswer { autonomy: Arc::clone(autonomy), answer: claimed, option_index })
}

/// The same for the desktop prompt-answer path (`AgentPromptAnswer`).
pub fn owner_prompt_answer_of(
    session_id: &str,
    prompt_id: &str,
    key: Option<&str>,
) -> Option<PendingOwnerAnswer> {
    let option_index = key?.trim().parse::<usize>().ok()?;
    let autonomy = installed()?;
    let claimed = autonomy.peek_owner_answer(session_id, prompt_id)?;
    Some(PendingOwnerAnswer { autonomy, answer: claimed, option_index })
}

/// One daemon observation: a blocked or prompting pane is read and handed to autonomy; any
/// other reading only clears the card of a prompt that is gone. The read runs in its own task
/// so the observer loop never waits on it.
pub fn observed(state: &Arc<RemoteGatewayState>, observation: &crate::agent_control::events::AgentObservation) {
    let Some(autonomy) = installed() else { return };
    let blocked = observation.prompt_id.is_some()
        || matches!(observation.state, crate::agent_control::AgentControlState::Blocked);
    if !blocked {
        let host = crate::remote::reference_chat::files::reference_host_id();
        autonomy.clear(&SessionRef::new(&host, &observation.session_id));
        return;
    }
    let (state, session_id) = (Arc::clone(state), observation.session_id.clone());
    tokio::spawn(async move { observe(&state, &session_id).await });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::remote::reference_chat::types::ReferencePromptAnswer;
    use ferryx_hub::supervisor::panel_contract::PendingPrompt;

    struct NoControl;

    #[async_trait]
    impl SessionControl for NoControl {
        async fn stop(&self, _: &SessionRef) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn send(&self, _: &SessionRef, _: &str) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn answer(&self, _: &SessionRef, _: &str, _: Option<&str>, _: usize) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn read_tail(&self, _: &SessionRef, _: usize) -> ferryx_hub::error::Result<String> {
            Ok(String::new())
        }
    }

    fn blocked(id: &str) -> FleetSession {
        FleetSession {
            session_ref: SessionRef::new("local", "s1"),
            host_id: "local".into(),
            host_label: "This computer".into(),
            title: "ferryx".into(),
            cwd: "/nonexistent/panel-autonomy-test".into(),
            agent: "claude".into(),
            state: "blocked".into(),
            state_source: "screen".into(),
            state_since_ms: None,
            screen_changed_at_ms: None,
            pending_prompt: Some(PendingPrompt {
                prompt_id: id.into(),
                kind: "question".into(),
                question: "Which package manager should I use?".into(),
                command: None,
                options: vec!["npm".into(), "pnpm".into()],
                screen_revision: Some(format!("rev-{id}")),
            }),
            last_assistant_line: None,
            updated_at_ms: 1,
            stale: false,
        }
    }

    fn answer_op(id: &str, request_id: Option<&str>) -> AgentControlOp {
        AgentControlOp::Answer {
            session_id: "s1".into(),
            prompt_id: id.into(),
            screen_revision: format!("rev-{id}"),
            answer: ReferencePromptAnswer { option_index: Some(1), ..Default::default() },
            request_id: request_id.map(str::to_owned),
        }
    }

    async fn next_of(
        frames: &mut tokio::sync::broadcast::Receiver<serde_json::Value>,
        kind: &str,
    ) -> serde_json::Value {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let frame = frames.recv().await.expect("the owner channel stays open");
                // The channel is process-wide; other tests publish on it too.
                let ours = frame["sessionRef"] == "local:s1"
                    || frame["action"]["sessionRef"] == "local:s1"
                    || frame["rules"].is_array();
                if frame["type"] == kind && ours {
                    return frame;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {kind} frame reached the owner channel"))
    }

    /// Real stores, real owner channel: a decision, an action and a rule change each reach the
    /// owner event channel, and an owner answer through the daemon's op path is learned once
    /// while the supervisor's own answer is not.
    #[tokio::test]
    async fn real_mutations_reach_the_owner_channel_and_owner_answers_are_learned() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let autonomy = Autonomy::open(pool.clone(), Arc::new(NoControl), Arc::new(OwnerChannel))
            .await
            .unwrap();
        let mut frames = super::super::subscribe_panel_events();

        autonomy.observe(&blocked("p1")).await;
        let opened = next_of(&mut frames, super::super::PANEL_DECISION_EVENT).await;
        assert_eq!(opened["sessionRef"], "local:s1");
        assert_eq!(opened["decision"]["prompt"]["question"], "Which package manager should I use?");
        let pending = autonomy.escalation_source().pending().await.unwrap();
        assert_eq!(pending.len(), 1);

        // A supervisor-marked answer is not the owner's.
        let supervisor = answer_op("p1", Some("sup-tool-abc"));
        assert!(owner_answer_with(&autonomy, &supervisor).is_none());
        // The owner's, through the same op, is claimed and recorded once it went through.
        let owner = answer_op("p1", Some("phone-1"));
        let claimed = owner_answer_with(&autonomy, &owner).expect("a tracked, unsettled prompt");
        claimed.recorded().await;
        // The log line is published before the card resolves.
        let action = next_of(&mut frames, super::super::PANEL_ACTION_EVENT).await;
        assert_eq!(action["action"]["summary"], "You chose pnpm");
        let resolved = next_of(&mut frames, super::super::PANEL_DECISION_EVENT).await;
        assert!(resolved["decision"].is_null(), "{resolved}");
        assert!(autonomy.escalation_source().pending().await.unwrap().is_empty());
        // The same answer reported twice is learned once.
        assert!(owner_answer_with(&autonomy, &owner).is_none());

        let rules = autonomy.rule_store();
        let first = rules.list().await.unwrap().remove(0);
        rules.upsert(first).await.unwrap();
        let rule_frame = next_of(&mut frames, super::super::PANEL_RULES_EVENT).await;
        assert_eq!(rule_frame["rules"].as_array().unwrap().len(), 4);
    }

    /// The owner channel is process-wide, so the test that watches it is the only one that
    /// publishes on it.
    struct Silent;

    impl PanelPublisher for Silent {
        fn decision(&self, _: &SessionRef, _: Option<&Escalation>) {}
        fn action(&self, _: &ActionEntry) {}
        fn rules(&self, _: &[StandingRule]) {}
    }

    #[tokio::test]
    async fn the_panel_snapshot_reads_the_installed_real_sources() {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let autonomy = Autonomy::open(pool, Arc::new(NoControl), Arc::new(Silent)).await.unwrap();
        autonomy.observe(&blocked("p2")).await;
        let sources = ferryx_hub::fleet::PanelSources {
            log: Some(autonomy.action_log()),
            rules: Some(autonomy.rule_store()),
            escalations: Some(autonomy.escalation_source()),
        };
        let snapshot = ferryx_hub::fleet::assemble_panel(&sources, vec![blocked("p2")], false, true, 0).await;
        assert_eq!(snapshot.decisions.len(), 1);
        assert_eq!(snapshot.rules.len(), 4);
        assert_eq!(snapshot.sessions.len(), 1);
        assert!(!snapshot.setup_required);
    }

    /// Epoch ms the fake clock starts at.
    const START_MS: i64 = 1_800_000_000_000;

    static CARD_INIT: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();
    static CARDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    /// An open recovery card (its prompt id) of the process-wide installed scheduler: four
    /// sessions on another computer stalled, were restarted once and stalled again, so each
    /// has a card. Every call hands out a card no other test holds.
    pub(crate) async fn take_recovery_card() -> String {
        CARD_INIT
            .get_or_init(|| async {
                let pool = ferryx_hub::storage::memory().await.unwrap();
                let autonomy = Autonomy::open(pool, Arc::new(NoControl), Arc::new(Silent)).await.unwrap();
                let stalled = |n: u32| {
                    let mut s = outage_session();
                    s.session_ref = SessionRef::new("mac-2", &format!("card{n}"));
                    s.state_source = "provider".into();
                    s.state_since_ms = Some(START_MS - 600_000);
                    s.screen_changed_at_ms = Some(START_MS - 600_000);
                    s
                };
                let clock = Arc::new(FakeClock::new(START_MS));
                let scheduler = build_recovery(
                    &autonomy,
                    Arc::new(ScriptedControl::default()),
                    Arc::new(FixedFleet { sessions: (1..=4).map(stalled).collect() }),
                    clock.clone(),
                );
                scheduler.tick().await;
                clock.advance(400_000);
                scheduler.tick().await;
                let cards = autonomy.escalation_source().pending().await.unwrap();
                assert_eq!(cards.len(), 4, "{cards:?}");
                *CARDS.lock().unwrap() = cards.into_iter().map(|c| c.prompt.prompt_id).collect();
                RECOVERY.set(scheduler).ok().expect("one installed scheduler per test process");
            })
            .await;
        CARDS.lock().unwrap().pop().expect("a recovery card per test")
    }

    /// Only a pass that left the lane running is a resume; a stop of any kind is not.
    #[test]
    fn a_goal_pass_that_stopped_the_lane_is_not_a_resume() {
        use ferryx_hub::goals::loop_driver::LoopOutcome;
        use ferryx_hub::goals::StopReason;
        let outcome = |stop_reason| LoopOutcome {
            lane_key: "lane".to_owned(),
            stop_reason,
            awaiting_approval: false,
            sent: false,
        };
        assert_eq!(retry_outcome_of(None), RetryOutcome::NothingToResume);
        assert_eq!(retry_outcome_of(Some(&outcome(None))), RetryOutcome::Resumed);
        for reason in [StopReason::SupervisorDegraded, StopReason::JudgeUnavailable] {
            assert_eq!(
                retry_outcome_of(Some(&outcome(Some(reason)))),
                RetryOutcome::Stopped(reason.to_string()),
                "{reason}"
            );
        }
    }

    /// A clock only the test moves.
    struct FakeClock {
        now_ms: std::sync::atomic::AtomicI64,
    }

    impl FakeClock {
        fn new(now_ms: i64) -> Self {
            Self { now_ms: std::sync::atomic::AtomicI64::new(now_ms) }
        }

        fn advance(&self, by_ms: i64) {
            self.now_ms.fetch_add(by_ms, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl Clock for FakeClock {
        fn now_ms(&self) -> i64 {
            self.now_ms.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// Records what recovery does to a session and serves its terminal tail; `outage` makes a
    /// proxy error appear after recovery has already seen the session.
    #[derive(Default)]
    struct ScriptedControl {
        stops: std::sync::Mutex<Vec<String>>,
        sends: std::sync::Mutex<Vec<(String, String)>>,
        fresh: std::sync::Mutex<String>,
    }

    impl ScriptedControl {
        fn outage(&self) {
            self.fresh.lock().unwrap().push_str("API Error: 502 Bad Gateway\n");
        }

        fn stops(&self) -> Vec<String> {
            self.stops.lock().unwrap().clone()
        }

        fn sends(&self) -> Vec<(String, String)> {
            self.sends.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl SessionControl for ScriptedControl {
        async fn stop(&self, session: &SessionRef) -> ferryx_hub::error::Result<()> {
            self.stops.lock().unwrap().push(session.as_str().to_owned());
            Ok(())
        }
        async fn send(&self, session: &SessionRef, text: &str) -> ferryx_hub::error::Result<()> {
            self.sends.lock().unwrap().push((session.as_str().to_owned(), text.to_owned()));
            Ok(())
        }
        async fn answer(&self, _: &SessionRef, _: &str, _: Option<&str>, _: usize) -> ferryx_hub::error::Result<()> {
            Ok(())
        }
        async fn read_tail(&self, _: &SessionRef, _: usize) -> ferryx_hub::error::Result<String> {
            Ok(format!("working on the relay\n{}", self.fresh.lock().unwrap()))
        }
    }

    /// A fleet that never changes.
    struct FixedFleet {
        sessions: Vec<FleetSession>,
    }

    #[async_trait]
    impl ferryx_hub::meta::mhc_supervisor::FleetSource for FixedFleet {
        async fn list(&self) -> ferryx_hub::error::Result<Vec<FleetSession>> {
            Ok(self.sessions.clone())
        }
    }

    /// A Claude Code session on another computer that is working through a proxy outage.
    fn outage_session() -> FleetSession {
        FleetSession {
            session_ref: SessionRef::new("mac-2", "s9"),
            host_id: "mac-2".into(),
            host_label: "Mac 2".into(),
            title: "relay".into(),
            cwd: "/nonexistent/recovery-test".into(),
            agent: "claude".into(),
            state: "working".into(),
            state_source: "screen".into(),
            state_since_ms: Some(START_MS),
            screen_changed_at_ms: Some(START_MS),
            pending_prompt: None,
            last_assistant_line: None,
            updated_at_ms: START_MS,
            stale: false,
        }
    }

    struct RecoveryFixture {
        autonomy: Arc<Autonomy>,
        scheduler: Arc<RecoveryScheduler>,
        control: Arc<ScriptedControl>,
        clock: Arc<FakeClock>,
    }

    /// The real scheduler over real autonomy stores (in-memory database), with a scripted
    /// session control, a fixed fleet and a fake clock. Recovery has seen the session once
    /// (its baseline pass) before the proxy outage starts: old scrollback is never news.
    async fn recovery_fixture() -> RecoveryFixture {
        let pool = ferryx_hub::storage::memory().await.unwrap();
        let autonomy = Autonomy::open(pool, Arc::new(NoControl), Arc::new(Silent)).await.unwrap();
        let control = Arc::new(ScriptedControl::default());
        let clock = Arc::new(FakeClock::new(START_MS));
        let scheduler = build_recovery(
            &autonomy,
            control.clone(),
            Arc::new(FixedFleet { sessions: vec![outage_session()] }),
            clock.clone(),
        );
        scheduler.tick().await;
        control.outage();
        RecoveryFixture { autonomy, scheduler, control, clock }
    }

    /// The real scheduler built by the daemon's wiring: a proxy outage on another computer's
    /// session is retried once after the first backoff, with one live log row for the incident.
    #[tokio::test]
    async fn the_built_scheduler_resumes_a_cross_host_session_after_the_first_backoff() {
        let fixture = recovery_fixture().await;

        fixture.scheduler.tick().await;
        assert!(fixture.control.sends().is_empty(), "nothing is retried before the first backoff");

        // The default `upstreamRetry` backoff starts at 60 s.
        fixture.clock.advance(61_000);
        fixture.scheduler.tick().await;

        assert_eq!(
            fixture.control.sends(),
            vec![("mac-2:s9".to_owned(), "continue".to_owned())]
        );
        assert!(fixture.control.stops().is_empty());
        let rows = fixture.autonomy.action_log().list_today(0).await.unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert!(rows[0].live, "{rows:?}");
    }

    /// A goal loop's quota fault is taken over by recovery: the report is accepted and the
    /// deferral shows as one live row.
    #[tokio::test]
    async fn a_goal_quota_fault_is_taken_over_and_logged_as_a_live_recovery_row() {
        let fixture = recovery_fixture().await;
        let quota = GoalQuota { scheduler: Arc::clone(&fixture.scheduler) };

        assert!(quota.quota_fault("lane-1", "s5", "429 usage limit reached").await);

        let rows = fixture.autonomy.action_log().list_today(0).await.unwrap();
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].incident_key, "recovery:goalquota:lane-1");
        assert!(rows[0].live, "{rows:?}");
        assert!(fixture.control.sends().is_empty());
    }
}
