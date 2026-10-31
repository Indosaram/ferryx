use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineAgentStateMessage {
    pub r#type: String,
    pub target: crate::remote::machine_protocol::RemoteTerminalTarget,
    pub state: String,
    pub agent: Option<String>,
    pub provider_session: Option<crate::daemon::protocol::AgentProviderSession>,
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_truncated: Option<bool>,
    /// `protocol` when the state came from the pane program's OSC 7501 report. Older peers ignore
    /// it and keep reading the frame as an agent report.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::daemon::protocol::deserialize_lenient_option"
    )]
    pub state_source: Option<crate::daemon::protocol::AgentStateWireSource>,
    /// How the turn behind an `idle` ended, when the producer said so.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::daemon::protocol::deserialize_lenient_option"
    )]
    pub outcome: Option<crate::daemon::protocol::AgentOutcome>,
    /// What a `blocked` session needs from the user, when the producer said so.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::daemon::protocol::deserialize_lenient_option"
    )]
    pub kind: Option<crate::daemon::protocol::AgentBlockedKind>,
}

impl MachineAgentStateMessage {
    pub const TYPE: &'static str = "agent_state";

    pub fn new(
        target: crate::remote::machine_protocol::RemoteTerminalTarget,
        state: String,
        agent: Option<String>,
        provider_session: Option<crate::daemon::protocol::AgentProviderSession>,
        detail: Option<String>,
    ) -> Self {
        Self {
            r#type: Self::TYPE.to_string(),
            target,
            state,
            agent,
            provider_session,
            detail,
            metadata_truncated: None,
            state_source: None,
            outcome: None,
            kind: None,
        }
    }

    pub fn truncated(
        target: crate::remote::machine_protocol::RemoteTerminalTarget,
        state: String,
        agent: Option<String>,
        provider_session: Option<crate::daemon::protocol::AgentProviderSession>,
    ) -> Self {
        Self {
            r#type: Self::TYPE.to_string(),
            target,
            state,
            agent,
            provider_session,
            detail: None,
            metadata_truncated: Some(true),
            state_source: None,
            outcome: None,
            kind: None,
        }
    }

    /// Carries the state's producer marker, outcome and blocked kind. They are a few bytes, so a
    /// truncated frame keeps them and drops only the detail and transcript path.
    pub(crate) fn with_report_marks(
        mut self,
        state: &crate::daemon::agent_state::AgentState,
    ) -> Self {
        self.state_source = state.origin.wire_source();
        self.outcome = state.outcome;
        self.kind = state.kind;
        self
    }

    /// The internal origin this frame stands for on the receiving side.
    pub fn origin(&self) -> crate::daemon::protocol::AgentStateOrigin {
        crate::daemon::protocol::AgentStateOrigin::from_wire(
            crate::daemon::protocol::AgentStateOrigin::Agent,
            self.state_source,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::agent_state::AgentState;
    use crate::daemon::protocol::{AgentBlockedKind, AgentOutcome, AgentStateOrigin};

    fn target() -> crate::remote::machine_protocol::RemoteTerminalTarget {
        crate::remote::machine_protocol::RemoteTerminalTarget {
            machine_id: "m-1".into(),
            daemon_epoch: crate::scoped_contracts::Epoch(7),
            session_id: "s-1".into(),
        }
    }

    fn protocol_state() -> AgentState {
        AgentState {
            session_id: "s-1".into(),
            state: "blocked".into(),
            agent: Some("claude".into()),
            provider_session: None,
            detail: Some("Allow edit?".into()),
            origin: AgentStateOrigin::Protocol,
            outcome: None,
            kind: Some(AgentBlockedKind::Question),
        }
    }

    /// The machine frame exactly as builds before the protocol marks decoded it.
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct PreProtocolMachineAgentState {
        r#type: String,
        target: crate::remote::machine_protocol::RemoteTerminalTarget,
        state: String,
        agent: Option<String>,
        provider_session: Option<crate::daemon::protocol::AgentProviderSession>,
        detail: Option<String>,
        #[serde(default)]
        metadata_truncated: Option<bool>,
    }

    #[test]
    fn the_protocol_marks_round_trip_and_an_older_peer_still_decodes_the_frame() {
        let state = protocol_state();
        let msg = MachineAgentStateMessage::new(
            target(),
            state.state.clone(),
            state.agent.clone(),
            None,
            state.detail.clone(),
        )
        .with_report_marks(&state);
        let json = serde_json::to_string(&msg).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["stateSource"], "protocol");
        assert_eq!(value["kind"], "question");
        assert!(value.get("outcome").is_none(), "{json}");
        let back: MachineAgentStateMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, msg);
        assert_eq!(back.origin(), AgentStateOrigin::Protocol);

        let old: PreProtocolMachineAgentState =
            serde_json::from_str(&json).expect("older peer decodes the new frame");
        assert_eq!(old.r#type, MachineAgentStateMessage::TYPE);
        assert_eq!(old.target, target());
        assert_eq!((old.state.as_str(), old.agent.as_deref()), ("blocked", Some("claude")));
        assert_eq!(old.detail.as_deref(), Some("Allow edit?"));
        assert!(old.provider_session.is_none() && old.metadata_truncated.is_none());
    }

    #[test]
    fn a_frame_without_marks_is_unchanged_and_decodes_as_an_agent_report() {
        let agent = AgentState { origin: AgentStateOrigin::Agent, kind: None, ..protocol_state() };
        let msg = MachineAgentStateMessage::new(target(), "idle".into(), None, None, None)
            .with_report_marks(&agent);
        let json = serde_json::to_string(&msg).unwrap();
        for key in ["stateSource", "outcome", "kind"] {
            assert!(!json.contains(key), "{json}");
        }
        let back: MachineAgentStateMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.origin(), AgentStateOrigin::Agent);
        // Unknown values from a newer peer read as absent instead of failing the frame.
        let mut newer: serde_json::Value = serde_json::from_str(&json).unwrap();
        newer["stateSource"] = "later".into();
        newer["outcome"] = "cancelled".into();
        let decoded: MachineAgentStateMessage = serde_json::from_value(newer).unwrap();
        assert_eq!((decoded.state_source, decoded.outcome), (None, None));
    }

    #[test]
    fn a_truncated_frame_keeps_the_marks() {
        let state = AgentState {
            state: "idle".into(),
            outcome: Some(AgentOutcome::Error),
            kind: None,
            ..protocol_state()
        };
        let msg = MachineAgentStateMessage::truncated(target(), state.state.clone(), None, None)
            .with_report_marks(&state);
        assert_eq!(msg.detail, None);
        assert_eq!(msg.outcome, Some(AgentOutcome::Error));
        assert_eq!(msg.origin(), AgentStateOrigin::Protocol);
    }
}
