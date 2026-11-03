//! Production wiring of the supervisor's autonomy adapters in the daemon (plan todos 5-8):
//! the real `SessionControl` / `FleetSource` behind the engine's session tools, the panel's
//! sources (action log, rules, escalations), the owner channel publishers, the answer history's
//! callers, and the pause flag. Everything here runs only for the owner's own processes: the
//! agent-control owner socket, the owner-authorized HTTP routes and the daemon's observer.
use std::sync::Arc;

use async_trait::async_trait;
use ferryx_hub::supervisor::adapters::{HostDirectory, PairedHost, RemoteFleetSource, SupervisorAgentControl};
use ferryx_hub::supervisor::autonomy::{Autonomy, PanelPublisher};
use ferryx_hub::supervisor::session_control::AgentSessionControl;
use ferryx_hub::supervisor::panel_contract::{
    ActionEntry, Escalation, FleetSession, SessionControl, SessionRef, StandingRule,
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
pub async fn install(launch: &HubLaunch) {
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
    let autonomy = match Autonomy::open(pool.clone(), session_control, Arc::new(OwnerChannel)).await {
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
mod tests {
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
}
