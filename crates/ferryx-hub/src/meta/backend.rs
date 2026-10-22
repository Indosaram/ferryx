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
        if self.url.trim().is_empty() {
            return Err(HubError::message("omo model backend URL is not configured"));
        }
        #[cfg(feature = "hub-slack")]
        {
            let engine = HeadlessEngine::new(
                WebsocketTransport {
                    url: self.url.clone(),
                },
                self.workspace.clone(),
            )
            .with_prompt(prompt);
            return engine.turn(prompt).await;
        }
        #[cfg(not(feature = "hub-slack"))]
        {
            let _ = prompt;
            Err(HubError::message(
                "omo websocket transport is compiled with hub-slack",
            ))
        }
    }
}

#[cfg(feature = "hub-slack")]
struct WebsocketTransport {
    url: String,
}

#[cfg(feature = "hub-slack")]
#[async_trait]
impl OmoTransport for WebsocketTransport {
    async fn roundtrip(&self, frames: Vec<String>) -> Result<Vec<Value>> {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;
        let (mut socket, _) = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio_tungstenite::connect_async(&self.url),
        )
        .await
        .map_err(|_| HubError::message("omo websocket connect timed out"))?
        .map_err(|error| HubError::message(error.to_string()))?;
        for frame in frames {
            socket
                .send(Message::Text(frame.into()))
                .await
                .map_err(|error| HubError::message(error.to_string()))?;
        }
        let mut events = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let next = tokio::time::timeout(remaining, socket.next()).await;
            let message = match next {
                Ok(Some(Ok(message))) => message,
                _ => break,
            };
            let Message::Text(text) = message else {
                continue;
            };
            if let Ok(value) = serde_json::from_str::<Value>(text.as_str()) {
                let done = value
                    .pointer("/params/item/text")
                    .and_then(Value::as_str)
                    .is_some();
                events.push(value);
                if done {
                    break;
                }
            }
        }
        Ok(events)
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
    async fn url_model_reads_the_assistant_event_from_the_omo_socket() {
        use crate::lanes::kibitzer::ModelTurn;
        use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
        use axum::response::IntoResponse;

        async fn handle(mut socket: WebSocket) {
            while let Some(Ok(message)) = socket.recv().await {
                let Message::Text(text) = message else {
                    continue;
                };
                if text.contains("turn/start") {
                    let body = r#"{"method":"item/agentMessage/delta","params":{"item":{"text":"{\"wake\":true,\"reason\":\"r\",\"suggested\":\"s\",\"target_lane\":null}"}}}"#;
                    let _ = socket.send(Message::Text(body.into())).await;
                    break;
                }
            }
        }
        async fn upgrade(ws: WebSocketUpgrade) -> impl IntoResponse {
            ws.on_upgrade(handle)
        }
        let app = axum::Router::new().route("/ws", axum::routing::get(upgrade));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let model = UrlModel::new(format!("ws://127.0.0.1:{port}/ws"));
        let text = model.complete("state").await.unwrap();
        let decision = crate::lanes::kibitzer::parse_decision(&text).unwrap();
        assert!(decision.wake);
    }
}
