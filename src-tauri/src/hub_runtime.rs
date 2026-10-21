//! Starts the always-on hub inside `ferryx-cli --daemon` and serves `ferryx-cli mcp`.
//!
//! The hub is not constructed by `DaemonServer::new`, so unit tests that build a daemon
//! do not open its database or a chat socket. Supervisor writes go through agent control,
//! which uses the shared reference-chat input queue and refuses a pane a person just typed in.
use std::sync::Arc;

use async_trait::async_trait;
use ferryx_hub::agent_control::{AgentControl, AgentOp, TaskLauncher};
use ferryx_hub::chat::session::SessionKey;
use ferryx_hub::security::{auto_answer, AutoDecision, Reading};
use ferryx_hub::sessions::watch::Observation;
use ferryx_hub::{Hub, HubConfig, HubError};
use serde_json::{json, Value};

use crate::agent_control::{AgentControlOp, AgentControlState, AgentStateSource};
use crate::daemon::client::DaemonClient;
use crate::daemon::protocol::{DaemonRequest, DaemonResponse};
use crate::daemon::server::DaemonServer;
use crate::remote::reference_chat::types::{ReferencePart, ReferencePromptAnswer, ReferenceTurn};

/// Maps a hub operation onto the daemon's agent-control request.
/// The request id is the one the caller supplied. A lost answer is not given a new id.
pub fn to_agent_control_op(op: &AgentOp) -> Result<AgentControlOp, String> {
    Ok(match op {
        AgentOp::List => AgentControlOp::List,
        AgentOp::Read { session_id } => AgentControlOp::Read { session_id: session_id.clone() },
        AgentOp::Send { session_id, text, request_id } => AgentControlOp::Send {
            session_id: session_id.clone(),
            text: text.clone(),
            request_id: Some(request_id.clone()),
        },
        AgentOp::Answer { session_id, prompt_id, screen_revision, answer, request_id } => {
            let answer: ReferencePromptAnswer = serde_json::from_value(answer.clone())
                .map_err(|error| format!("answer: {error}"))?;
            if !answer.is_single_choice() {
                return Err("an answer must be exactly one of optionIndex, optionIndices, or customText".into());
            }
            AgentControlOp::Answer {
                session_id: session_id.clone(),
                prompt_id: prompt_id.clone(),
                screen_revision: screen_revision.clone(),
                answer,
                request_id: Some(request_id.clone()),
            }
        }
        AgentOp::Stop { session_id, request_id } => AgentControlOp::Stop {
            session_id: session_id.clone(),
            request_id: Some(request_id.clone()),
        },
        AgentOp::Wait { session_id, until, after_revision, after, timeout_ms } => AgentControlOp::Wait {
            session_id: session_id.clone(),
            until: until.clone(),
            after_revision: after_revision.clone(),
            after: *after,
            timeout_ms: *timeout_ms,
        },
        AgentOp::Events { after } => AgentControlOp::Events { after: *after },
    })
}

/// Text lines from a ferryx transcript parser. Callers that only have a screen use the
/// screen-revision diff in `ferryx_hub::sessions::external` instead.
pub fn history_lines(agent: &str, transcript: &str) -> Vec<String> {
    let turns = match agent {
        "claude" => crate::remote::reference_chat::history_claude::parse_claude_transcript(transcript).unwrap_or_default(),
        "codex" => crate::remote::reference_chat::history_parse::parse_codex_transcript(transcript),
        "omo" => {
            let mut titles = std::collections::HashMap::new();
            crate::remote::reference_chat::history_omo::parse_omo_transcript(transcript, 40, false, &mut titles)
        }
        _ => Vec::new(),
    };
    lines_of(&turns)
}

fn lines_of(turns: &[ReferenceTurn]) -> Vec<String> {
    turns
        .iter()
        .flat_map(|turn| {
            turn.parts.iter().filter_map(|part| match part {
                ReferencePart::Text { text, .. } | ReferencePart::Thinking { text } => Some(text.clone()),
                _ => None,
            })
        })
        .filter(|text| !text.trim().is_empty())
        .collect()
}

struct DaemonBridge {
    client: DaemonClient,
}

#[async_trait]
impl AgentControl for DaemonBridge {
    async fn execute(&self, host_id: Option<&str>, op: AgentOp) -> Result<Value, HubError> {
        let request = to_agent_control_op(&op).map_err(HubError::message)?;
        let host_id = host_id.filter(|host| *host != "local" && !host.is_empty()).map(str::to_owned);
        let response = self
            .client
            .send_request(DaemonRequest::AgentControl { request, host_id })
            .await
            .map_err(|error| HubError::message(error.to_string()))?;
        match response {
            DaemonResponse::AgentControlOk { result } => Ok(result),
            DaemonResponse::Error { message, code, .. } => {
                Err(HubError::message(format!("{} {message}", code.unwrap_or_default())))
            }
            _ => Err(HubError::message("the daemon returned an unexpected agent-control response")),
        }
    }
}

#[async_trait]
impl TaskLauncher for DaemonBridge {
    async fn launch_pane(&self, cwd: &str, command: &[String]) -> Result<String, HubError> {
        let workspace = std::env::var("FERRYX_HUB_WORKSPACE").map_err(|_| {
            HubError::config("FERRYX_HUB_WORKSPACE is required before the hub can launch a pane")
        })?;
        let shell = if command.is_empty() { None } else { Some(command.join(" ")) };
        self.client
            .spawn_terminal(format!("hub-{}", std::process::id()), workspace, None, Some(cwd.to_owned()), 100, 32, shell)
            .await
            .map_err(|error| HubError::message(error.to_string()))
    }
}

/// Opens the hub after the daemon has announced ready. Failures are returned to the caller,
/// which logs them and leaves the daemon up.
pub async fn start(server: Arc<DaemonServer>) -> Result<(), String> {
    if std::env::var("FERRYX_HUB").ok().as_deref() == Some("off") {
        return Ok(());
    }
    let config = HubConfig::load().map_err(|error| error.to_string())?;
    #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
    let profiles = ferryx_hub::chat::router::ProfileRouter::new(config.profile_routes.clone());
    #[cfg(any(feature = "hub-slack", feature = "hub-discord"))]
    let meta_session_id = config.meta_session_id.clone().unwrap_or_else(|| "meta".into());
    let bridge = Arc::new(DaemonBridge { client: DaemonClient::new() });
    let hub = Hub::open(config.clone(), bridge.clone()).await.map_err(|error| error.to_string())?;
    if let Err(error) = ferryx_hub::chat::reply::flush_pending(&hub.pool).await {
        tracing::warn!(%error, "stored chat replies were not flushed");
    }
    if config.supervisor_pane {
        let preset = ferryx_hub::meta::pane::SupervisorPanePreset::from_parts(
            config.supervisor_cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
            config.supervisor_command.clone(),
        );
        tracing::info!(title = %preset.title, "supervisor pane preset is armed");
        if !preset.command.is_empty() {
            if let Err(error) = bridge.launch_pane(&preset.cwd.to_string_lossy(), &preset.command).await {
                tracing::warn!(%error, "supervisor pane was not launched");
            }
        }
    }
    let mut events = server.remote_state().agent_events.subscribe();
    let watch_hub = Arc::clone(&hub);
    let watch_bridge = bridge.clone();
    tokio::spawn(async move {
        let mut answered = std::collections::HashSet::new();
        loop {
            match events.recv().await {
                Ok(observation) => {
                    if let Err(error) = on_agent_event(&watch_hub, watch_bridge.as_ref(), &mut answered, observation).await {
                        tracing::debug!(%error, "hub observer event was not applied");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    if let Some(addr) = config.tracker_addr.clone() {
        let token = config.tracker_owner_token.clone().unwrap_or_default();
        let router = ferryx_hub::tracker::Tracker::new(hub.pool.clone(), token).router();
        tokio::spawn(async move {
            match tokio::net::TcpListener::bind(&addr).await {
                Ok(listener) => {
                    if let Err(error) = axum::serve(listener, router).await {
                        tracing::warn!(%error, "tracker stopped");
                    }
                }
                Err(error) => tracing::warn!(%error, addr, "tracker did not bind"),
            }
        });
    }
    #[cfg(feature = "hub-slack")]
    if ferryx_hub::chat::slack::app_token().is_some() {
        let owner = config.owner_slack_id.clone().unwrap_or_default();
        let slack_bridge = bridge.clone();
        let slack_profiles = profiles.clone();
        let slack_meta = meta_session_id.clone();
        tokio::spawn(async move {
            let owner_for_frames = owner.clone();
            if let Err(error) = ferryx_hub::chat::slack::connect_socket_mode(move |text| {
                let bridge = slack_bridge.clone();
                let profiles = slack_profiles.clone();
                let meta = slack_meta.clone();
                let owner = owner_for_frames.clone();
                async move {
                    if let Err(error) = ferryx_hub::chat::slack::apply_frame(&text, &owner, &meta, &profiles, bridge.as_ref()).await {
                        tracing::warn!(%error, "slack frame was not applied");
                    }
                }
            })
            .await
            {
                tracing::warn!(%error, "slack socket mode stopped");
            }
        });
    }
    #[cfg(feature = "hub-discord")]
    if ferryx_hub::chat::discord::bot_token().is_some() {
        let discord_bridge = bridge.clone();
        let discord_profiles = profiles.clone();
        let discord_meta = meta_session_id.clone();
        tokio::spawn(async move {
            if let Err(error) = ferryx_hub::chat::discord::connect_gateway(move |frame| {
                let bridge = discord_bridge.clone();
                let profiles = discord_profiles.clone();
                let meta = discord_meta.clone();
                async move {
                    if let Err(error) = ferryx_hub::chat::discord::apply_dispatch(&frame, &meta, &profiles, bridge.as_ref()).await {
                        tracing::warn!(%error, "discord frame was not applied");
                    }
                }
            })
            .await
            {
                tracing::warn!(%error, "discord gateway stopped");
            }
        });
    }
    tracing::info!("ferryx hub is up");
    Ok(())
}

async fn on_agent_event(
    hub: &Hub,
    control: &DaemonBridge,
    answered: &mut std::collections::HashSet<String>,
    observation: crate::agent_control::events::AgentObservation,
) -> Result<(), HubError> {
    let session_id = observation.session_id.clone();
    let mut question = None;
    let mut screen_readable = observation.screen_revision.is_some() && observation.state_source != AgentStateSource::None;
    if observation.prompt_id.is_some() {
        if let Ok(value) = control.execute(None, AgentOp::Read { session_id: session_id.clone() }).await {
            question = value.pointer("/session/pending/question").and_then(Value::as_str).map(str::to_owned);
            if value.pointer("/session/screen/text").and_then(Value::as_str).unwrap_or("").is_empty() {
                screen_readable = false;
            }
        }
    }
    let target = format!("local:{session_id}");
    let watched = ferryx_hub::sessions::watch::Watcher::is_watched(&hub.pool, &target).await?;
    let obs = Observation {
        target: target.clone(),
        machine: "local".into(),
        session_id: session_id.clone(),
        name: session_id.clone(),
        message_count: observation.sequence,
        streaming: matches!(observation.state, AgentControlState::Working),
        blocking_question: matches!(observation.state, AgentControlState::Blocked) && question.is_none(),
        question: question.clone(),
        last_assistant: String::new(),
        watched,
        machine_failed: false,
    };
    hub.ingest(&obs).await?;
    let reading = Reading {
        state_source: match observation.state_source {
            AgentStateSource::Provider => "provider".into(),
            AgentStateSource::Screen => "screen".into(),
            AgentStateSource::None => "none".into(),
        },
        kind: observation.kind.clone(),
        screen_readable,
    };
    let suggested = question.unwrap_or_default();
    if let (Some(prompt_id), Some(revision)) = (observation.prompt_id.clone(), observation.screen_revision.clone()) {
        if answered.insert(prompt_id.clone()) {
            match auto_answer(&reading, &suggested) {
                AutoDecision::Answer { .. } => {
                    let _ = control
                        .execute(
                            None,
                            AgentOp::Answer {
                                session_id,
                                prompt_id: prompt_id.clone(),
                                screen_revision: revision,
                                answer: json!({"optionIndex": 0}),
                                request_id: format!("hub-auto-{prompt_id}"),
                            },
                        )
                        .await;
                }
                AutoDecision::EscalateOwner => {
                    let session = SessionKey::new("slack", None::<String>, "owner", None::<String>, "owner");
                    let _ = ferryx_hub::chat::reply::chat_reply(
                        &hub.pool,
                        &session,
                        &format!("approval {prompt_id} on {target} needs the owner"),
                        None,
                    )
                    .await;
                }
                AutoDecision::Hold => {}
            }
        }
    }
    Ok(())
}

/// `ferryx-cli mcp` — a stdio MCP server over this machine's daemon.
pub fn run_mcp() -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("ferryx mcp: {error}");
            return 1;
        }
    };
    runtime.block_on(async {
        let bridge = Arc::new(DaemonBridge { client: DaemonClient::new() });
        let config = HubConfig::load().unwrap_or_default();
        let path = ferryx_hub::hub::default_database_path();
        let pool = match ferryx_hub::storage::connect(&path).await {
            Ok(pool) => pool,
            Err(error) => {
                eprintln!("ferryx mcp: {error}");
                return 1;
            }
        };
        let context = Arc::new(ferryx_hub::mcp::McpContext {
            control: bridge.clone(),
            launcher: bridge,
            pool,
            meta_session_id: config.meta_session_id.unwrap_or_else(|| "meta".into()),
            tracker_token: config.tracker_owner_token.unwrap_or_default(),
        });
        if let Err(error) = ferryx_hub::mcp::serve(context).await {
            eprintln!("ferryx mcp: {error}");
            return 1;
        }
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_and_codex_history_parsers_feed_the_hub_transcript() {
        let claude = history_lines("claude", "");
        assert!(claude.is_empty());
        let codex = history_lines("codex", "");
        assert!(codex.is_empty());
    }

    #[test]
    fn an_answer_op_keeps_the_callers_request_id() {
        let op = AgentOp::Answer {
            session_id: "pane".into(),
            prompt_id: "prompt".into(),
            screen_revision: "rev".into(),
            answer: json!({"optionIndex": 0}),
            request_id: "slack-approval-prompt".into(),
        };
        let mapped = to_agent_control_op(&op).unwrap();
        match mapped {
            AgentControlOp::Answer { request_id, screen_revision, answer, .. } => {
                assert_eq!(request_id.as_deref(), Some("slack-approval-prompt"));
                assert_eq!(screen_revision, "rev");
                assert_eq!(answer.option_index, Some(0));
            }
            other => panic!("expected an answer, got {other:?}"),
        }
    }
}
