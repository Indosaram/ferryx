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
}

impl<T: OmoTransport> HeadlessEngine<T> {
    pub fn new(transport: T, workspace: Workspace) -> Self {
        Self {
            transport,
            workspace,
            system_prompt: None,
            model: None,
        }
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
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
                None,
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
}
