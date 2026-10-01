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
        }
    }
}
