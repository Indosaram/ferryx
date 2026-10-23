//! Headless omo engine. A transport stands in for the websocket so tests do not spawn a daemon.
use async_trait::async_trait;
use serde_json::Value;

use super::protocol::{self, Workspace};
use crate::error::{HubError, Result};

#[async_trait]
pub trait OmoTransport: Send + Sync {
    async fn roundtrip(&self, frames: Vec<String>) -> Result<Vec<Value>>;
}

pub struct HeadlessEngine<T> {
    transport: T,
    workspace: Workspace,
    system_prompt: Option<String>,
    model: Option<String>,
    toolsets: Vec<String>,
}

impl<T: OmoTransport> HeadlessEngine<T> {
    pub fn new(transport: T, workspace: Workspace) -> Self {
        Self {
            transport,
            workspace,
            system_prompt: None,
            model: None,
            toolsets: Vec::new(),
        }
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_toolsets(mut self, toolsets: Vec<String>) -> Self {
        self.toolsets = toolsets;
        self
    }

    /// initialize, thread/start, turn/start. Assistant text is collected from events.
    /// Approval requests are declined. The reply is the text the engine returns, not a screen scrape.
    pub async fn turn(&self, prompt: &str) -> Result<String> {
        let frames = vec![
            protocol::initialize_request(),
            protocol::thread_start_request(
                self.system_prompt.as_deref(),
                self.model.as_deref(),
                Some(&self.workspace),
                (!self.toolsets.is_empty()).then_some(self.toolsets.as_slice()),
            ),
            protocol::turn_start_request("pending", prompt, self.model.as_deref()),
        ];
        let events = self.transport.roundtrip(frames).await?;
        let mut thread_id = None;
        let mut parts = Vec::new();
        for event in &events {
            if event.get("method").and_then(Value::as_str) == Some("thread/start") {
                thread_id = event
                    .pointer("/result/threadId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            if let Some(id) = event.pointer("/result/thread/id").and_then(Value::as_str) {
                thread_id = Some(id.to_owned());
            }
            if let Some(text) = event.pointer("/params/item/text").and_then(Value::as_str) {
                parts.push(text.to_owned());
            }
            if let Some(method) = event.get("method").and_then(Value::as_str) {
                if protocol::is_approval_request(method) {
                    let _ =
                        protocol::approval_denial_response(event.get("id").unwrap_or(&Value::Null));
                }
            }
        }
        if parts.is_empty() {
            return Err(HubError::message(format!(
                "omo turn produced no assistant text (thread {thread_id:?})"
            )));
        }
        Ok(parts.join("\n"))
    }
}

/// Websocket client for the omo app-server. Kibitzer and `/omo-compress` call it
/// when `FERRYX_HUB_OMO_URL` is set.
pub struct UrlModel {
    url: String,
    #[cfg(feature = "hub-slack")]
    workspace: Workspace,
}

impl UrlModel {
    pub fn new(url: impl Into<String>) -> Self {
        #[cfg(feature = "hub-slack")]
        let cwd = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .display()
            .to_string();
        Self {
            url: url.into(),
            #[cfg(feature = "hub-slack")]
            workspace: Workspace {
                roots: vec![cwd.clone()],
                cwd,
            },
        }
    }
}

#[async_trait]
impl crate::lanes::kibitzer::ModelTurn for UrlModel {
    async fn complete(&self, prompt: &str) -> Result<String> {
        self.complete_with(prompt, None, &[]).await
    }

    async fn complete_with(
        &self,
        prompt: &str,
        model: Option<&str>,
        toolsets: &[String],
    ) -> Result<String> {
        if self.url.trim().is_empty() {
            return Err(HubError::message("omo model backend URL is not configured"));
        }
        #[cfg(feature = "hub-slack")]
        {
            // Same sequence omo-gateway uses (each reply awaited, Bearer token from the
            // supervised daemon's token file, events read until turn/completed).
            let mut config =
                super::omo_client::OmoClientConfig::new(self.url.clone()).with_env_timeouts();
            config.workspace = Some(self.workspace.clone());
            config.token = super::supervisor::state_dir_from_home().and_then(|dir| {
                let probe =
                    super::supervisor::DaemonSupervisor::new(self.url.clone(), Vec::new(), dir);
                super::omo_client::token_from_file(&probe.token_path())
            });
            let request = super::omo_client::TurnRequest {
                thread_id: None,
                system_prompt: Some(prompt.to_owned()),
                model: model.map(str::to_owned),
                toolsets: (!toolsets.is_empty()).then(|| toolsets.to_vec()),
                text: prompt.to_owned(),
            };
            super::omo_client::run_turn(&config, &request, None)
                .await
                .map(|outcome| outcome.text)
        }
        #[cfg(not(feature = "hub-slack"))]
        {
            let _ = (prompt, model, toolsets);
            Err(HubError::message(
                "omo websocket transport is compiled with hub-slack",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Scripted(Vec<Value>);

    #[async_trait]
    impl OmoTransport for Scripted {
        async fn roundtrip(&self, frames: Vec<String>) -> Result<Vec<Value>> {
            assert_eq!(frames.len(), 3);
            assert!(frames[0].contains("initialize"));
            assert!(frames[1].contains("thread/start"));
            assert!(frames[2].contains("turn/start"));
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn a_turn_returns_assistant_text_from_events() {
        let engine = HeadlessEngine::new(
            Scripted(vec![json!({
                "method": "item/agentMessage/delta",
                "params": {"item": {"text": "ready"}}
            })]),
            Workspace {
                cwd: "/tmp".into(),
                roots: vec!["/tmp".into()],
            },
        );
        let reply = engine.turn("hello").await.unwrap();
        assert_eq!(reply, "ready");
    }

    #[cfg(feature = "hub-slack")]
    #[tokio::test]
    async fn url_model_runs_a_full_omo_turn() {
        use crate::lanes::kibitzer::ModelTurn;
        use crate::meta::omo_client::mock::{serve, Script};
        let decision_json = r#"{"wake":true,"reason":"r","suggested":"s","target_lane":null}"#;
        let (url, log) = serve(Script {
            messages: vec![decision_json.into()],
            resume_error: None,
            approval: false,
            thread_id: "thr-k".into(),
            queued_ticks: 0,
            tick: std::time::Duration::ZERO,
            no_ack: false,
        })
        .await;
        let model = UrlModel::new(url);
        let text = model.complete("state").await.unwrap();
        let decision = crate::lanes::kibitzer::parse_decision(&text).unwrap();
        assert!(decision.wake);
        let frames = log.frames.lock().unwrap().clone();
        let methods: Vec<_> = frames
            .iter()
            .filter_map(|f| f.get("method").and_then(Value::as_str))
            .collect();
        assert_eq!(methods, ["initialize", "thread/start", "turn/start"]);
        assert_eq!(frames[2]["params"]["threadId"], "thr-k");
    }
}
