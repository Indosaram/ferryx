use super::*;
use crate::agent_control::{events::AgentEventBus, AgentControlState, AgentStateSource, AgentTier};

#[test]
fn production_observation_sink_persists_published_events_and_recovers_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let catalog_path = dir.path().join("catalog.json");
    let workspaces = Arc::new(crate::daemon::workspace_service::DaemonWorkspaceService::new(
        crate::worktree::WorkspaceRegistry::new(),
        catalog_path,
    ));
    let bus = Arc::new(AgentEventBus::new(crate::agent_control::events::AGENT_EVENT_RETAIN));
    let writer = install_agent_stamp_persistence(&bus, &workspaces).expect("journal writer");

    let draft = |state| crate::agent_control::events::ObservationDraft {
        session_id: "test-session".into(),
        state,
        state_source: AgentStateSource::Screen,
        tier: AgentTier::Screen,
        prompt_id: None,
        kind: None,
        screen_revision: Some(format!("screen-{state:?}")),
        workspace_id: None,
    };
    let published = [
        bus.publish(draft(AgentControlState::Working)),
        bus.publish(draft(AgentControlState::Blocked)),
    ];
    let epoch = bus.instance().to_owned();

    bus.clear_stamp_sink();
    writer.join().expect("journal writer exits after draining its queue");

    let reopened = crate::remote::machine_operation_journal::MachineOperationJournal::open(
        dir.path().join("machine-operations.v1.json"),
    );
    let history = reopened.observation_history().unwrap();
    assert_eq!(history.instance.as_deref(), Some(epoch.as_str()));
    assert_eq!(
        history.retained.iter().map(|row| row.observation.clone()).collect::<Vec<_>>(),
        published,
        "the production sink persists every actual bus publication"
    );

    let recovered = AgentEventBus::new(crate::agent_control::events::AGENT_EVENT_RETAIN);
    assert!(recovered.adopt_history(history));
    assert_eq!(recovered.snapshot(), vec![published[1].clone()]);
    assert_ne!(recovered.instance(), epoch, "recovery starts a fresh cursor epoch");
}
