//! Daemon relay transport, deliberately independent of the SSH helper runtime.
//! The owner drives receive and keepalive; no task, process or PTY is spawned here.
use super::output_hub::TerminalOutputHub;
use crate::{
    paired_host::{
        client::{ClientError, MachineClient},
        inventory::CredentialLease,
        service::PairedHostService,
    },
    remote::{
        machine_protocol as m,
        terminal_wire::{decode_frame, Metadata},
    },
    scoped_contracts::Epoch,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};
type Result<T> = std::result::Result<T, ClientError>;
/// Bound on one relay->direct upgrade. Input queued in the owner mailbox while
/// it runs must still be answered well inside the 30s operation deadline.
const UPGRADE_TIMEOUT: Duration = Duration::from_secs(10);
fn error(code: &str) -> ClientError {
    ClientError::local(code)
}

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
    /// Direct-route epoch this socket actually rides; `None` on the relay.
    pub(crate) direct_epoch: Option<u64>,
    /// Host-scoped pid the host reported for the attached session.
    pub(crate) remote_pid: Option<crate::ssh::bridge::RemotePid>,
}
/// Live identity of a paired proxy as its owner actor last observed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairedLive {
    pub remote_pid: Option<crate::ssh::bridge::RemotePid>,
    /// False once the host reported the remote process exited.
    pub reader_open: bool,
    /// True while a transport and controller generation are held.
    pub connected: bool,
}
impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transport").finish()
    }
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
    exited: bool,
    /// Credential authority captured by the last explicit reattach; used only to
    /// recover from a lost direct route, never to create a session.
    recovery: Option<PairedHostService>,
    agent_sink: Option<Arc<dyn crate::terminal::remote::AgentStateSink>>,
    remote_pid: Option<crate::ssh::bridge::RemotePid>,
    desired_geometry: Option<(u16, u16)>,
    reconnect_delay: Duration,
    blocked: Option<String>,
    exit_code: Option<i32>,
}
#[cfg(feature = "local-split-qa")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedRouteReceipt {
    pub selected: String,
    pub websocket: bool,
    pub target: m::RemoteTerminalTarget,
    pub generation: Option<Epoch>,
    pub direct_epoch: Option<u64>,
    pub loopback: bool,
    pub after_sequence: Option<Epoch>,
}
impl Proxy {
    pub fn new(descriptor: Descriptor, hub: Arc<TerminalOutputHub>) -> Result<Self> {
        let id = m::proxy_backend_id(&descriptor.host_id, &descriptor.target)
            .map_err(|_| error("INVALID_REQUEST"))?;
        // R4-N3: live-transport ownership is fenced separately from the retained
        // output entry. A retained hub (transport lost without confirmed exit) is
        // adoptable by the next proxy instead of conflicting; only a live owner conflicts.
        if !hub.claim_transport(&id) {
            return Err(error("CONTROL_CONFLICT"));
        }
        // Adopting a retained entry must keep its buffered history and live
        // subscribers: re-registering would reset the bounded buffer and drop
        // the old broadcast sender.
        if !hub.has_session(&id) {
            hub.register_session(&id);
        }
        Ok(Self {
            id,
            descriptor,
            hub,
            transport: None,
            controller: None,
            replay_pending: false,
            exited: false,
            recovery: None,
            agent_sink: None,
            remote_pid: None,
            desired_geometry: None,
            reconnect_delay: Duration::from_millis(500),
            blocked: None,
            exit_code: None,
        })
    }
    pub fn set_agent_sink(&mut self, sink: Arc<dyn crate::terminal::remote::AgentStateSink>) {
        self.agent_sink = Some(sink);
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn descriptor(&self) -> &Descriptor {
        &self.descriptor
    }
    pub fn controller(&self) -> Option<Epoch> {
        self.controller
    }
    pub fn live_state(&self) -> PairedLive {
        PairedLive {
            remote_pid: self.remote_pid,
            reader_open: !self.exited,
            connected: self.transport.is_some() && self.controller.is_some(),
        }
    }
    pub(crate) fn exited(&self) -> bool {
        self.exited
    }
    pub(crate) fn set_recovery(&mut self, service: PairedHostService) {
        self.recovery = Some(service);
    }
    pub(crate) fn subscribe_authority(&self) -> Option<tokio::sync::watch::Receiver<(u64, String)>> {
        self.recovery.as_ref().map(PairedHostService::subscribe_authority)
    }
    pub(crate) fn outcome(&self) -> (&str, Option<i32>) {
        if self.exited {
            ("affirmative_exit", self.exit_code)
        } else if self.blocked.is_some() {
            ("blocked", None)
        } else if self.controller.is_some() {
            ("proxy_attached", None)
        } else {
            ("owner_unavailable", None)
        }
    }
    pub(crate) fn blocked_reason(&self) -> Option<&str> {
        self.blocked.as_deref()
    }
    pub(crate) fn rearm(&mut self) {
        self.blocked = None;
    }
    pub(crate) fn retry_admission(&mut self) -> std::result::Result<(), &'static str> {
        if self.exited { return Err("PAIRED_PROXY_EXITED"); }
        if self.blocked.is_some() { return Err("PAIRED_PROXY_BLOCKED"); }
        if self.controller.is_none() { self.rearm(); }
        Ok(())
    }
    pub(crate) async fn rearm_authority(&mut self, host: &str) -> bool {
        if host != self.descriptor.host_id || self.exited || !matches!(self.blocked.as_deref(), Some("PAIRED_HOST_UNAUTHORIZED" | "UNAUTHORIZED" | "PERMISSION_DENIED" | "STALE_GENERATION" | "PAIRED_HOST_STALE_GENERATION")) {
            return false;
        }
        let Some(service) = &self.recovery else { return false; };
        if service.current_generation(self.descriptor.host_id.clone(), self.descriptor.generation).await.is_err() {
            return false;
        }
        self.rearm();
        true
    }
    pub(crate) fn recovery_failed(&mut self, failure: &ClientError) {
        self.mark_disconnected();
        match failure.code.as_str() {
            "PAIRED_SESSION_EXITED" => self.exited = true,
            "SESSION_EXPIRED" | "CONTROL_CONFLICT" | "STALE_GENERATION" | "PAIRED_HOST_STALE_GENERATION"
            | "PAIRED_HOST_UNAUTHORIZED" | "UNAUTHORIZED" | "PERMISSION_DENIED"
            | "PAIRED_HOST_WRONG_MACHINE" | "PAIRED_HOST_INVALID_RESPONSE"
            | "PAIRED_HOST_MIGRATION_PENDING" | "UNSUPPORTED_PROTOCOL" => {
                self.blocked = Some(failure.code.clone());
            }
            _ => {}
        }
    }

    /// Explicit reattach only. A failed connection never submits CreateSession.
    pub async fn reattach(
        &mut self,
        client: &MachineClient,
        service: &PairedHostService,
    ) -> Result<()> {
        self.attach(client, service, true).await
    }

    pub(crate) async fn recover(&mut self) -> Result<()> {
        let Some(service) = self.recovery.clone() else {
            return Err(error("PAIRED_HOST_UNAVAILABLE"));
        };
        tokio::time::timeout(Duration::from_secs(30), self.attach(&MachineClient::new(), &service, false))
            .await.map_err(|_| error("TIMEOUT"))?
    }

    async fn attach(
        &mut self,
        client: &MachineClient,
        service: &PairedHostService,
        explicit: bool,
    ) -> Result<()> {
        if explicit {
            self.detach().await?;
        }
        self.recovery = Some(service.clone());
        let mut transport = client.attach_terminal(service, &self.descriptor).await?;
        let Message::Text(text) = transport.receive().await? else {
            return Err(error("PAIRED_HOST_INVALID_RESPONSE"));
        };
        if let Ok(m::Control::Error { error: rejection }) = serde_json::from_str(&text) {
            return Err(ClientError { code: rejection.code.clone(), machine_error: Some(rejection), request_id: None, ambiguous: false });
        }
        let m::Attached::Attached {
            target,
            generation,
            cols,
            rows,
            ..
        } = serde_json::from_str(&text).map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
        if target != self.descriptor.target {
            return Err(error("PAIRED_HOST_WRONG_MACHINE"));
        }
        transport.lease.token()?;
        service.current_generation(self.descriptor.host_id.clone(), self.descriptor.generation).await?;
        transport.lease.token()?;
        if cols == 0 || rows == 0 || cols > 1000 || rows > 1000 {
            return Err(error("PAIRED_HOST_INVALID_RESPONSE"));
        }
        if let Some((cols, rows)) = self.desired_geometry {
            transport.send(Message::Text(serde_json::json!({"type":"resize","generation":generation,"cols":cols,"rows":rows}).to_string().into())).await?;
        }
        self.hub.record_initial_size(&self.id, cols, rows);
        self.controller = Some(generation);
        self.blocked = None;
        self.reconnect_delay = Duration::from_millis(500);
        self.replay_pending = true;
        if transport.remote_pid.is_some() {
            self.remote_pid = transport.remote_pid;
        }
        self.transport = Some(transport);
        Ok(())
    }
    pub(crate) fn recovery_delay(&self) -> Duration {
        self.reconnect_delay
    }
    pub(crate) fn backoff_recovery(&mut self) {
        self.reconnect_delay = self.reconnect_delay.saturating_mul(2).min(Duration::from_secs(30));
    }
    pub(crate) fn can_recover(&self) -> bool {
        !self.exited && self.blocked.is_none() && self.controller.is_none() && self.recovery.is_some()
    }
    pub(crate) fn mark_disconnected(&mut self) {
        self.transport = None;
        self.controller = None;
    }
    /// A live stream riding the relay (no direct epoch) that may move to direct.
    pub(crate) fn relay_attached(&self) -> bool {
        !self.exited
            && self.blocked.is_none()
            && self.controller.is_some()
            && self.transport.as_ref().is_some_and(|t| t.direct_epoch.is_none())
    }
    /// Make-before-break move of a live relay stream onto direct route `epoch`.
    ///
    /// The relay transport stays installed while the direct socket attaches from
    /// the retained cursor; it is replaced (and dropped) only after the direct
    /// handshake is fully validated, so a failure leaves the relay stream as it
    /// was. The caller does not read the relay meanwhile, so the direct replay
    /// starts exactly after the last published byte. Never CreateSession, never
    /// replays input, never resets the hub. Returns whether the stream is now on
    /// a direct route.
    pub(crate) async fn upgrade_direct(&mut self, epoch: u64) -> Result<bool> {
        let routes = &crate::paired_host::direct_route::GLOBAL_DIRECT_ROUTES;
        if !self.relay_attached() || routes.direct_for(&self.descriptor.host_id).map(|(_, e)| e) != Some(epoch) {
            return Ok(false);
        }
        let Some(service) = self.recovery.clone() else {
            return Ok(false);
        };
        let result = tokio::time::timeout(UPGRADE_TIMEOUT, self.attach(&MachineClient::new(), &service, false))
            .await
            .map_err(|_| error("TIMEOUT"))
            .and_then(|attached| attached);
        if let Err(failure) = &result {
            if crate::paired_host::direct_client::is_transport_failure(failure) {
                routes.mark_disconnected(&self.descriptor.host_id, epoch, std::time::Instant::now());
            }
        }
        result.map(|()| self.transport.as_ref().is_some_and(|t| t.direct_epoch.is_some()))
    }
    #[cfg(feature = "local-split-qa")]
    pub(crate) fn qa_route_receipt(&self) -> std::result::Result<PairedRouteReceipt, &'static str> {
        let transport = self.transport.as_ref().ok_or("OWNER_UNAVAILABLE")?;
        let peer = match transport.socket.get_ref() {
            MaybeTlsStream::Plain(socket) => socket.peer_addr(),
            MaybeTlsStream::Rustls(socket) => socket.get_ref().0.peer_addr(),
            _ => return Err("QA_LOOPBACK_REQUIRED"),
        }.map_err(|_| "OWNER_UNAVAILABLE")?;
        Ok(PairedRouteReceipt {
            selected: if transport.direct_epoch.is_some() { "direct" } else { "relay" }.into(),
            websocket: true,
            target: self.descriptor.target.clone(),
            generation: self.controller,
            direct_epoch: transport.direct_epoch,
            loopback: peer.ip().is_loopback(),
            after_sequence: self.descriptor.after_sequence,
        })
    }
    #[cfg(feature = "local-split-qa")]
    pub(crate) fn qa_drop_loopback_transport(&mut self) -> std::result::Result<(), &'static str> {
        let transport = self.transport.as_ref().ok_or("OWNER_UNAVAILABLE")?;
        let peer = match transport.socket.get_ref() {
            MaybeTlsStream::Plain(socket) => socket.peer_addr(),
            MaybeTlsStream::Rustls(socket) => socket.get_ref().0.peer_addr(),
            _ => return Err("QA_LOOPBACK_REQUIRED"),
        }.map_err(|_| "OWNER_UNAVAILABLE")?;
        if !peer.ip().is_loopback() { return Err("QA_LOOPBACK_REQUIRED"); }
        self.mark_disconnected();
        Ok(())
    }
    pub(crate) fn remember_geometry(&mut self, cols: u16, rows: u16) {
        if cols > 0 && rows > 0 && cols <= 1000 && rows <= 1000 {
            self.desired_geometry = Some((cols, rows));
        }
    }
    pub(crate) fn agent_sink(&self) -> Option<Arc<dyn crate::terminal::remote::AgentStateSink>> {
        self.agent_sink.clone()
    }
    pub async fn detach(&mut self) -> Result<()> {
        self.controller = None;
        // Dropping the exact socket preempts IO; no remote close command is sent.
        self.transport = None;
        Ok(())
    }
    async fn send(&mut self, generation: Epoch, message: Message) -> Result<()> {
        if self.controller.is_none() {
            return Err(error("PAIRED_NOT_SENT"));
        }
        if self.controller != Some(generation) {
            return Err(error("STALE_GENERATION"));
        }
        let result = self
            .transport
            .as_mut()
            .ok_or_else(|| error("HOST_UNAVAILABLE"))?
            .send(message)
            .await;
        if let Err(failure) = &result {
            let direct = self.transport.as_ref().and_then(|t| t.direct_epoch);
            self.transport = None;
            self.controller = None;
            // The failed message is never resent; recovery only restores the
            // stream so the owner loop keeps receiving under a new generation.
            self.retire_direct_loss(direct, failure);
        }
        result
    }
    /// A direct route died under a live stream: retire that route epoch and
    /// reattach from the retained cursor (the path manager now picks the relay or
    /// a newer direct route). Never CreateSession, never replays input, never
    /// resets the hub. Relay loss, remote exit and host answers keep old semantics.
    fn retire_direct_loss(&mut self, direct: Option<u64>, failure: &ClientError) {
        let Some(epoch) = direct else {
            return;
        };
        if self.exited || !crate::paired_host::direct_client::is_transport_failure(failure) {
            return;
        }
        crate::paired_host::direct_route::GLOBAL_DIRECT_ROUTES.mark_disconnected(
            &self.descriptor.host_id,
            epoch,
            std::time::Instant::now(),
        );
        self.transport = None;
        self.controller = None;
    }
    pub async fn write(&mut self, generation: Epoch, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 64 * 1024 {
            return Err(error("PAYLOAD_TOO_LARGE"));
        }
        self.send(generation, Message::Binary(bytes.to_vec().into()))
            .await
            .map_err(|mut failure| {
                // Bytes may have reached the host before the socket failed.
                if crate::paired_host::direct_client::is_transport_failure(&failure) {
                    failure.ambiguous = true;
                    failure.code = "PAIRED_UNCERTAIN_SEND".into();
                }
                failure
            })
    }
    pub async fn resize(&mut self, generation: Epoch, cols: u16, rows: u16) -> Result<()> {
        if cols == 0 || rows == 0 || cols > 1000 || rows > 1000 {
            return Err(error("INVALID_REQUEST"));
        }
        if self.controller.map_or(generation != Epoch(0), |current| generation != current) {
            return Err(error("STALE_GENERATION"));
        }
        self.remember_geometry(cols, rows);
        self.send(generation, Message::Text(serde_json::json!({"type":"resize","generation":generation,"cols":cols,"rows":rows}).to_string().into())).await?;
        self.hub.record_resize(&self.id, cols, rows);
        self.desired_geometry = Some((cols, rows));
        Ok(())
    }
    pub async fn interrupt(&mut self, generation: Epoch) -> Result<()> {
        self.interrupt_with_intent(generation, None).await
    }
    pub async fn interrupt_with_intent(&mut self, generation: Epoch, intent_id: Option<String>) -> Result<()> {
        let mut message = serde_json::json!({"type":"signal","generation":generation,"signal":"interrupt"});
        if let Some(intent_id) = intent_id {
            message["intentId"] = serde_json::Value::String(intent_id);
        }
        self.send(
            generation,
            Message::Text(
                message.to_string()
                    .into(),
            ),
        )
        .await.map_err(|mut failure| {
            if crate::paired_host::direct_client::is_transport_failure(&failure) {
                failure.ambiguous = true;
                failure.code = "PAIRED_UNCERTAIN_SEND".into();
            }
            failure
        })
    }
    /// A keepalive that tripped a recovered direct loss reports success: the
    /// stream lives on under the new controller generation.
    pub async fn ping(&mut self, generation: Epoch) -> Result<()> {
        let before = self.controller;
        match self
            .send(generation, Message::Text("{\"type\":\"ping\"}".into()))
            .await
        {
            Err(_) if self.controller.is_some() && self.controller != before => Ok(()),
            result => result,
        }
    }
    /// One bounded message at a time; consumers subscribe to the native hub.
    /// Lifecycle JSON is returned to the owner and never rendered as PTY bytes.
    pub async fn receive(&mut self) -> Result<Option<serde_json::Value>> {
        let direct = self.transport.as_ref().and_then(|t| t.direct_epoch);
        let result = self.receive_inner().await;
        if let Err(failure) = &result {
            self.transport = None;
            self.controller = None;
            self.retire_direct_loss(direct, failure);
        }
        result
    }
    async fn receive_inner(&mut self) -> Result<Option<serde_json::Value>> {
        let message = self
            .transport
            .as_mut()
            .ok_or_else(|| error("HOST_UNAVAILABLE"))?
            .receive()
            .await?;
        match message {
            Message::Binary(bytes) => {
                let frame =
                    decode_frame(&bytes).map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
                let (end, gap) = match frame.metadata {
                    Metadata::Replay { end, gap, .. } if self.replay_pending => (end, gap),
                    Metadata::Output { sequence, gap } => (Some(sequence), gap),
                    _ => return Err(error("PAIRED_HOST_INVALID_RESPONSE")),
                };
                self.replay_pending = false;
                if gap.is_none()
                    && end.is_some_and(|end| {
                        self.descriptor
                            .after_sequence
                            .is_some_and(|last| end <= last.0)
                    })
                {
                    return Ok(None);
                }
                if gap.is_some() {
                    self.hub.publish_gap(&self.id);
                }
                self.hub.publish(&self.id, frame.terminal_bytes.to_vec());
                if let Some(end) = end {
                    self.descriptor.after_sequence = Some(Epoch(end));
                }
                Ok(None)
            }
            Message::Text(text) => {
                if text.len() > 16 * 1024 {
                    return Err(error("PAYLOAD_TOO_LARGE"));
                }
                let value: serde_json::Value = serde_json::from_str(&text)
                    .map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
                if let Some(target) = value.get("target") {
                    let target: m::RemoteTerminalTarget = serde_json::from_value(target.clone())
                        .map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
                    if target != self.descriptor.target {
                        return Err(error("PAIRED_HOST_WRONG_MACHINE"));
                    }
                }
                if matches!(value["type"].as_str(), Some("exit")) {
                    let m::Control::Exit { target, exit } = serde_json::from_value(value.clone())
                        .map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))? else {
                            return Err(error("PAIRED_HOST_INVALID_RESPONSE"));
                        };
                    if target != self.descriptor.target {
                        return Err(error("PAIRED_HOST_WRONG_MACHINE"));
                    }
                    self.exited = true;
                    self.exit_code = exit.code;
                    self.controller = None;
                    self.transport = None;
                } else if value["type"].as_str() == Some("error") {
                    let m::Control::Error { error: rejection } = serde_json::from_value(value.clone())
                        .map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))? else {
                            return Err(error("PAIRED_HOST_INVALID_RESPONSE"));
                        };
                    return Err(ClientError { code: rejection.code.clone(), machine_error: Some(rejection), request_id: None, ambiguous: false });
                } else if value["type"].as_str() == Some("status") {
                    let m::Control::Status { target, status } = serde_json::from_value(value.clone())
                        .map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))? else {
                            return Err(error("PAIRED_HOST_INVALID_RESPONSE"));
                        };
                    if target != self.descriptor.target { return Err(error("PAIRED_HOST_WRONG_MACHINE")); }
                    match status {
                        m::SessionStatus::Running => {},
                        m::SessionStatus::Exited => {
                            self.exited = true;
                            self.mark_disconnected();
                        }
                        m::SessionStatus::Expired => {
                            self.mark_disconnected();
                            self.blocked = Some("SESSION_EXPIRED".into());
                        }
                    }
                } else if matches!(value["type"].as_str(), Some("agent_state")) {
                    // Required fields that do not decode are a wire-contract violation,
                    // not a message to drop: a silent skip would lose authoritative
                    // state while the stream still looks healthy.
                    let msg = serde_json::from_value::<crate::remote::machine_agent_state::MachineAgentStateMessage>(value.clone())
                        .map_err(|_| error("PAIRED_HOST_INVALID_RESPONSE"))?;
                    // Exact compare kept as a backstop: the shared target check above
                    // only runs when the frame carries `target`, so this frame must
                    // still prove it belongs to this descriptor before its state lands.
                    if msg.target != self.descriptor.target {
                        return Err(error("PAIRED_HOST_WRONG_MACHINE"));
                    }
                    if let Some(sink) = &self.agent_sink {
                        sink.accept(crate::daemon::agent_state::AgentState {
                            session_id: self.id.clone(),
                            state: msg.state,
                            agent: msg.agent,
                            provider_session: msg.provider_session,
                            detail: msg.detail,
                            origin: crate::daemon::protocol::AgentStateOrigin::Agent,
                        });
                    }
                }
                Ok(Some(value))
            }
            Message::Ping(bytes) => {
                self.transport
                    .as_mut()
                    .ok_or_else(|| error("HOST_UNAVAILABLE"))?
                    .send(Message::Pong(bytes))
                    .await?;
                Ok(None)
            }
            Message::Pong(_) => Ok(None),
            _ => Err(error("HOST_UNAVAILABLE")),
        }
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.hub.release_transport(&self.id);
        if self.exited {
            self.hub.remove_session(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor() -> Descriptor {
        Descriptor {
            host_id: "host-key".into(),
            generation: Epoch(1),
            target: m::RemoteTerminalTarget {
                machine_id: "m".into(),
                daemon_epoch: Epoch(1),
                session_id: "s".into(),
            },
            after_sequence: None,
        }
    }

    #[test]
    fn proxy_reclaims_retained_hub_entry_after_unconfirmed_exit() {
        let hub = Arc::new(TerminalOutputHub::new(1024));
        let first = Proxy::new(descriptor(), hub.clone()).unwrap();
        assert!(Proxy::new(descriptor(), hub.clone()).is_err());
        drop(first);
        let second = Proxy::new(descriptor(), hub.clone()).unwrap();
        assert!(hub.has_session(second.id()));
        assert!(Proxy::new(descriptor(), hub.clone()).is_err());
        drop(second);
    }
}
