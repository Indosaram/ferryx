//! The prompt an agent pane is asking, read and answered from the desktop. The daemon owns the
//! panes, so both commands go to it; it runs the same detectors and the same screen-validated
//! answer path the remote chat uses.

use std::sync::Arc;

use tauri::State;

use crate::daemon::protocol::{AgentPromptAnswerInput, AgentPromptView};
use crate::daemon::DaemonClient;
use crate::ipc::IpcError;
use crate::scoped_contracts::DeliveryReceipt;

#[tauri::command]
pub async fn cmd_agent_prompt_get(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    registry_id: Option<String>,
) -> Result<Option<AgentPromptView>, IpcError> {
    daemon_client
        .inner()
        .clone()
        .agent_prompt_get(session_id, registry_id)
        .await
}

#[tauri::command]
pub async fn cmd_agent_prompt_answer(
    daemon_client: State<'_, Arc<DaemonClient>>,
    session_id: String,
    prompt_id: String,
    screen_revision: String,
    answer: AgentPromptAnswerInput,
    registry_id: Option<String>,
) -> Result<DeliveryReceipt, IpcError> {
    daemon_client
        .inner()
        .clone()
        .agent_prompt_answer(session_id, prompt_id, screen_revision, answer, registry_id)
        .await
}
