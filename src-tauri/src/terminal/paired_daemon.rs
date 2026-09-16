//! Daemon relay transport, deliberately independent of the SSH helper runtime.
//! The owner drives receive and keepalive; no task, process or PTY is spawned here.
use crate::{paired_host::{client::{ClientError, MachineClient}, inventory::CredentialLease, service::PairedHostService}, remote::{machine_protocol as m, terminal_wire::{decode_frame, Metadata}}, scoped_contracts::Epoch};
use super::output_hub::TerminalOutputHub;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, tungstenite::Message};
type Result<T> = std::result::Result<T, ClientError>;
fn error(code: &str) -> ClientError { ClientError::local(code) }

/// Credential-free durable identity. Local output cursors are never remote cursors.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Descriptor {
    pub host_id: String,
    pub generation: Epoch,
    pub target: m::RemoteTerminalTarget,
    pub after_sequence: Option<Epoch>,
}
pub struct Transport {
    pub(crate) socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    pub(crate) lease: CredentialLease,
}
impl Transport {
    async fn receive(&mut self) -> Result<Message> {
        self.lease.token()?;
        let mut cancelled = self.lease.cancellation();
        tokio::select! { biased;
            _ = cancelled.changed() => Err(error("PAIRED_HOST_STALE_GENERATION")),
            result = tokio::time::timeout(Duration::from_secs(60), self.socket.next()) =>
                result.map_err(|_| error("TIMEOUT"))?.ok_or_else(|| error("HOST_UNAVAILABLE"))?.map_err(|_| error("HOST_UNAVAILABLE")),
        }
    }
    async fn send(&mut self, message: Message) -> Result<()> {
        self.lease.token()?;
        let mut cancelled = self.lease.cancellation();
        tokio::select! { biased;
            _ = cancelled.changed() => Err(error("PAIRED_HOST_STALE_GENERATION")),
            result = tokio::time::timeout(Duration::from_secs(30), self.socket.send(message)) =>
                result.map_err(|_| error("TIMEOUT"))?.map_err(|_| error("HOST_UNAVAILABLE")),
        }
    }
}

pub struct Proxy {
    id: String,
    descriptor: Descriptor,
    hub: Arc<TerminalOutputHub>,
    transport: Option<Transport>,
    controller: Option<Epoch>,
    replay_pending: bool,
}
impl Proxy {
    pub fn new(descriptor: Descriptor, hub: Arc<TerminalOutputHub>) -> Result<Self> {
        let id = m::proxy_backend_id(&descriptor.host_id, &descriptor.target).map_err(|_| error("INVALID_REQUEST"))?;
        if hub.has_session(&id) { return Err(error("CONTROL_CONFLICT")); }
        hub.register_session(&id);
        Ok(Self { id, descriptor, hub, transport: None, controller: None, replay_pending: false })
    }
    pub fn id(&self) -> &str { &self.id }
    pub fn descriptor(&self) -> &Descriptor { &self.descriptor }
    pub fn controller(&self) -> Option<Epoch> { self.controller }

    /// Explicit reattach only. A failed connection never submits CreateSession.
    pub async fn reattach(&mut self, client: &MachineClient, service: &PairedHostService) -> Result<()> {
        self.detach().await?;
        let mut transport = client.attach_terminal(service, &self.descriptor).await?;
        let Message::Text(text) = transport.receive().await? else { return Err(error("PAIRED_HOST_INVALID_RESPONSE")); };
        let m::Attached::Attached { target, generation, cols, rows, .. } = serde_json::from_str(&text).map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
        if target != self.descriptor.target { return Err(error("PAIRED_HOST_WRONG_MACHINE")); }
        self.hub.record_initial_size(&self.id, cols, rows);
        self.controller = Some(generation);
        self.replay_pending = true;
        self.transport = Some(transport);
        Ok(())
    }
    pub async fn detach(&mut self) -> Result<()> {
        self.controller = None;
        if let Some(mut transport) = self.transport.take() { transport.send(Message::Close(None)).await?; }
        Ok(())
    }
    async fn send(&mut self, generation: Epoch, message: Message) -> Result<()> {
        if self.controller != Some(generation) { return Err(error("STALE_GENERATION")); }
        let result = self.transport.as_mut().ok_or_else(|| error("HOST_UNAVAILABLE"))?.send(message).await;
        if result.is_err() { self.transport = None; self.controller = None; }
        result
    }
    pub async fn write(&mut self, generation: Epoch, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 64 * 1024 { return Err(error("PAYLOAD_TOO_LARGE")); }
        self.send(generation, Message::Binary(bytes.to_vec().into())).await
    }
    pub async fn resize(&mut self, generation: Epoch, cols: u16, rows: u16) -> Result<()> {
        if cols == 0 || rows == 0 || cols > 1000 || rows > 1000 { return Err(error("INVALID_REQUEST")); }
        self.send(generation, Message::Text(serde_json::json!({"type":"resize","generation":generation,"cols":cols,"rows":rows}).to_string().into())).await?;
        self.hub.record_resize(&self.id, cols, rows);
        Ok(())
    }
    pub async fn interrupt(&mut self, generation: Epoch) -> Result<()> {
        self.send(generation, Message::Text(serde_json::json!({"type":"signal","generation":generation,"signal":"interrupt"}).to_string().into())).await
    }
    pub async fn ping(&mut self, generation: Epoch) -> Result<()> {
        self.send(generation, Message::Text("{\"type\":\"ping\"}".into())).await
    }
    /// One bounded message at a time; consumers subscribe to the native hub.
    /// Lifecycle JSON is returned to the owner and never rendered as PTY bytes.
    pub async fn receive(&mut self) -> Result<Option<serde_json::Value>> {
        let result = self.receive_inner().await;
        if result.is_err() { self.transport = None; self.controller = None; }
        result
    }
    async fn receive_inner(&mut self) -> Result<Option<serde_json::Value>> {
        let message = self.transport.as_mut().ok_or_else(|| error("HOST_UNAVAILABLE"))?.receive().await?;
        match message {
            Message::Binary(bytes) => {
                let frame = decode_frame(&bytes).map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
                let (end, gap) = match frame.metadata {
                    Metadata::Replay { end, gap, .. } if self.replay_pending => (end, gap),
                    Metadata::Output { sequence, gap } => (Some(sequence), gap),
                    _ => return Err(error("PAIRED_HOST_INVALID_RESPONSE")),
                };
                self.replay_pending = false;
                if gap.is_none() && end.is_some_and(|end| self.descriptor.after_sequence.is_some_and(|last| end <= last.0)) { return Ok(None); }
                if gap.is_some() { self.hub.publish_gap(&self.id); }
                self.hub.publish(&self.id, frame.terminal_bytes.to_vec());
                if let Some(end) = end { self.descriptor.after_sequence = Some(Epoch(end)); }
                Ok(None)
            }
            Message::Text(text) => {
                if text.len() > 16 * 1024 { return Err(error("PAYLOAD_TOO_LARGE")); }
                let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
                if let Some(target) = value.get("target") {
                    let target: m::RemoteTerminalTarget = serde_json::from_value(target.clone()).map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
                    if target != self.descriptor.target { return Err(error("PAIRED_HOST_WRONG_MACHINE")); }
                }
                if matches!(value["type"].as_str(), Some("exit" | "status" | "error")) { self.controller = None; self.transport = None; }
                Ok(Some(value))
            }
            Message::Ping(bytes) => {
                self.transport.as_mut().ok_or_else(|| error("HOST_UNAVAILABLE"))?.send(Message::Pong(bytes)).await?;
                Ok(None)
            }
            Message::Pong(_) => Ok(None),
            _ => Err(error("HOST_UNAVAILABLE")),
        }
    }
}
impl Drop for Proxy {
    fn drop(&mut self) { self.hub.remove_session(&self.id); }
}
