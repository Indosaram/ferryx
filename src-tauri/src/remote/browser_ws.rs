//! Remote Browser WebSocket Per-Connection Lifetime, Actors & Dispatcher
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.3, §4.4, §5, §6.2)

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use crate::remote::auth::DevicePermission;
use crate::remote::browser_admission::{AdmissionController, SubscriberQueue, CONTROL_QUEUE_MAX_COUNT};
use crate::remote::browser_backend::{
    BrowserCommandContext, RemoteBrowserBackend, RemoteBrowserError,
};
use crate::remote::browser_protocol::{ClientMessage, ServerMessage};
use crate::remote::browser_security::{
    require_permission, sanitize_public_string, sanitize_url, RequestDeduplicator,
    MAX_FILL_BYTES, MAX_REQUEST_WIRE_BYTES, MAX_SCRIPT_BYTES,
};

pub fn sanitize_json_value(val: serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => serde_json::Value::String(sanitize_public_string(&s)),
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(sanitize_json_value).collect())
        }
        serde_json::Value::Object(map) => {
            let mut new_map = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                new_map.insert(sanitize_public_string(&k), sanitize_json_value(v));
            }
            serde_json::Value::Object(new_map)
        }
        other => other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsConnectionState {
    Opening,
    Ready,
    Streaming,
    Paused,
    Closing,
    Closed,
}

pub struct BrowserWsSession {
    pub connection_id: String,
    pub device_id: String,
    pub browser_id: String,
    pub permission: DevicePermission,
    pub state: WsConnectionState,
    pub is_driver: bool,
    pub lease_epoch: Option<u64>,
    pub subscription_id: Option<String>,
    pub stream_id: u32,
    pub queue: Option<SubscriberQueue>,
    pub backend_subscription_id: Option<String>,
    pub dedup: Arc<RequestDeduplicator>,
    pub eval_semaphore: Arc<Semaphore>,
    pub command_semaphore: Arc<Semaphore>,
    pub cancel_token: CancellationToken,
    pub last_heartbeat: Instant,
    pub pending_promoted_frame: Option<Vec<u8>>,
}

fn browser_error(
    request_id: Option<String>,
    code: impl Into<String>,
    message: impl AsRef<str>,
    retryable: bool,
    retry_after_ms: Option<u64>,
) -> ServerMessage {
    ServerMessage::BrowserError {
        request_id,
        code: code.into(),
        message: sanitize_public_string(message.as_ref()),
        retryable,
        retry_after_ms,
    }
}

impl BrowserWsSession {
    pub fn new(
        connection_id: String,
        device_id: String,
        browser_id: String,
        permission: DevicePermission,
        now: Instant,
    ) -> Self {
        Self {
            connection_id,
            device_id,
            browser_id,
            permission,
            state: WsConnectionState::Opening,
            is_driver: false,
            lease_epoch: None,
            subscription_id: None,
            stream_id: 1,
            queue: None,
            backend_subscription_id: None,
            dedup: Arc::new(RequestDeduplicator::new()),
            eval_semaphore: Arc::new(Semaphore::new(1)),
            command_semaphore: Arc::new(Semaphore::new(CONTROL_QUEUE_MAX_COUNT)),
            cancel_token: CancellationToken::new(),
            last_heartbeat: now,
            pending_promoted_frame: None,
        }
    }

    pub fn enqueue_frame(&mut self, frame_bytes: Vec<u8>, now: Instant) -> Option<Vec<u8>> {
        if self.state == WsConnectionState::Paused {
            return None;
        }
        let queue = self.queue.as_mut()?;
        let seq = if frame_bytes.len() >= 8 {
            u32::from_le_bytes(frame_bytes[4..8].try_into().unwrap_or([0; 4]))
        } else {
            0
        };
        match queue.enqueue_frame(seq, frame_bytes.clone(), now) {
            crate::remote::browser_admission::AdmissionOutcome::Admit => Some(frame_bytes),
            _ => None,
        }
    }

    pub async fn dispatch_raw_text(
        &mut self,
        text: &str,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        out_tx: &tokio::sync::mpsc::Sender<ServerMessage>,
        now: Instant,
    ) -> Result<(), String> {
        if text.len() > MAX_REQUEST_WIRE_BYTES {
            let err = browser_error(
                None,
                "BROWSER_INVALID_REQUEST",
                "Raw WebSocket message exceeds wire size limit of 64 KiB",
                false,
                None,
            );
            let _ = out_tx.send(err).await;
            return Ok(());
        }

        match serde_json::from_str::<ClientMessage>(text) {
            Ok(client_msg) => {
                self.dispatch_client_message(client_msg, backend, admission, out_tx, now).await
            }
            Err(e) => {
                let err = browser_error(
                    None,
                    "BROWSER_INVALID_REQUEST",
                    format!("Invalid JSON request: {}", e),
                    false,
                    None,
                );
                let _ = out_tx.send(err).await;
                Ok(())
            }
        }
    }

    pub async fn handle_raw_text(
        &mut self,
        text: &str,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        now: Instant,
    ) -> Result<Option<ServerMessage>, String> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        self.dispatch_raw_text(text, backend, admission, &tx, now).await?;
        Ok(rx.recv().await)
    }

    pub fn handle_client_binary(&mut self, _bytes: &[u8]) -> Result<(), String> {
        // §4.2: Client to server binary frames are strictly rejected
        Err(sanitize_public_string("Client-to-server binary WebSocket frames are rejected"))
    }

    pub async fn dispatch_client_message(
        &mut self,
        msg: ClientMessage,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        out_tx: &tokio::sync::mpsc::Sender<ServerMessage>,
        now: Instant,
    ) -> Result<(), String> {
        let limiter = admission.get_device_limiter(&self.device_id, now);

        match msg {
            ClientMessage::BrowserSubscribe {
                request_id,
                viewer_instance_id,
                options,
            } => {
                let sub_id = format!("sub-{}", self.connection_id);
                if let Err(e) = admission.try_subscribe(
                    &self.browser_id,
                    &sub_id,
                    &self.device_id,
                    &viewer_instance_id,
                ) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_SUBSCRIPTION_FAILED",
                        format!("Subscription failed: {e}"),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // Wire subscription ownership and reconcile stream identity with backend
                match backend
                    .subscribe_viewer(&self.browser_id, &self.device_id, &viewer_instance_id)
                    .await
                {
                    Ok((backend_sub, stream_id)) => {
                        self.backend_subscription_id = Some(backend_sub);
                        self.stream_id = stream_id;
                    }
                    Err(e) => {
                        admission.unsubscribe(&self.browser_id, &sub_id);
                        let err = browser_error(
                            Some(request_id),
                            "BROWSER_SUBSCRIPTION_FAILED",
                            format!("Backend subscription failed: {e}"),
                            false,
                            None,
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                }

                let queue = SubscriberQueue::new(sub_id.clone(), self.stream_id);
                self.queue = Some(queue);
                self.subscription_id = Some(sub_id.clone());
                self.state = WsConnectionState::Streaming;

                let resp = ServerMessage::BrowserSubscribed {
                    request_id,
                    subscription_id: sub_id,
                    stream_id: self.stream_id,
                    browser_id: self.browser_id.clone(),
                    browser_instance_id: "bi1".into(),
                    browser_service_epoch: "1".into(),
                    desktop_epoch: "1".into(),
                    document_generation: "1".into(),
                    options,
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserFrameAck { stream_id, seq } => {
                if self.state != WsConnectionState::Paused {
                    if let Some(queue) = self.queue.as_mut() {
                        if let Some((_promoted_seq, promoted_bytes)) = queue.acknowledge_frame(stream_id, seq, now) {
                            self.pending_promoted_frame = Some(promoted_bytes);
                        }
                    }
                }
                Ok(())
            }

            ClientMessage::BrowserHeartbeat {
                request_id,
                lease_epoch,
                subscription_id,
            } => {
                self.last_heartbeat = now;
                if let Some(ref epoch_str) = lease_epoch {
                    let sub_id = self.subscription_id.as_deref().unwrap_or("");
                    let epoch = epoch_str.parse::<u64>().unwrap_or(0);
                    let sub_matches = subscription_id.as_deref() == Some(sub_id);
                    let epoch_matches = self.lease_epoch == Some(epoch);
                    if !self.is_driver
                        || !sub_matches
                        || !epoch_matches
                        || admission
                            .broker
                            .refresh_lease(
                                &self.device_id,
                                &self.connection_id,
                                sub_id,
                                &self.browser_id,
                                epoch,
                                now,
                            )
                            .is_err()
                    {
                        let err = browser_error(
                            request_id,
                            "BROWSER_INVALID_REQUEST",
                            "Driver lease 5-tuple mismatch or lease expired",
                            false,
                            None,
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                }
                let resp = ServerMessage::BrowserPong {
                    request_id,
                    timestamp: Some(now.elapsed().as_secs_f64()),
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserDriverClaim {
                request_id,
                subscription_id,
                browser_id,
            } => {
                if let Err(e) = require_permission(self.permission, DevicePermission::Control) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_FORBIDDEN",
                        e.to_string(),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // P1-04: Reject BrowserDriverClaim if client has not subscribed yet
                // or if subscription_id / browser_id does not match
                if self.subscription_id.is_none()
                    || self.subscription_id.as_deref() != Some(&subscription_id)
                    || browser_id != self.browser_id
                {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Subscription binding mismatch or not subscribed",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if !limiter.check_claim(now) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_RATE_LIMITED",
                        "Driver claim rate limit exceeded",
                        true,
                        Some(500),
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                match admission.broker.claim_driver(
                    &self.device_id,
                    &self.connection_id,
                    &subscription_id,
                    &browser_id,
                    now,
                ) {
                    Ok(lease) => {
                        self.is_driver = true;
                        self.lease_epoch = Some(lease.lease_epoch);
                        let resp = ServerMessage::BrowserDriverClaimed {
                            request_id,
                            lease_epoch: lease.lease_epoch.to_string(),
                            expires_at: lease.expires_at.elapsed().as_secs_f64(),
                        };
                        let _ = out_tx.send(resp).await;
                        Ok(())
                    }
                    Err(code) => {
                        let err = browser_error(
                            Some(request_id),
                            code,
                            "Another remote driver is active",
                            true,
                            Some(1000),
                        );
                        let _ = out_tx.send(err).await;
                        Ok(())
                    }
                }
            }

            ClientMessage::BrowserDriverRelease {
                request_id,
                lease_epoch,
            } => {
                let epoch = lease_epoch.parse::<u64>().unwrap_or(0);
                let sub_id = self.subscription_id.as_deref().unwrap_or("");
                let epoch_matches = self.lease_epoch == Some(epoch);
                if !self.is_driver
                    || !epoch_matches
                    || !admission.broker.release_driver(
                        &self.device_id,
                        &self.connection_id,
                        sub_id,
                        &self.browser_id,
                        epoch,
                    )
                {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Driver lease 5-tuple mismatch or not active driver",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }
                self.is_driver = false;
                self.lease_epoch = None;
                let resp = ServerMessage::BrowserDriverReleased {
                    request_id,
                    lease_epoch: Some(lease_epoch),
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserCommand {
                request_id,
                request_seq,
                browser_id,
                lease_epoch,
                browser_instance_id: _,
                desktop_epoch: _,
                document_generation,
                command,
                params,
            } => {
                // P1-04: Reject BrowserCommand if browser_id != self.browser_id or self.subscription_id.is_none()
                if browser_id != self.browser_id || self.subscription_id.is_none() {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Invalid browser or subscription binding",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                // P1-06: Boundary size limits on incoming commands:
                // command name <= 64 KiB, script in params <= 32 KiB, fill text <= 16 KiB
                if command.len() > MAX_REQUEST_WIRE_BYTES {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_INVALID_REQUEST",
                        "Command name exceeds limit",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if let Some(ref p) = params {
                    if let Some(script) = p.get("script").and_then(|v| v.as_str()) {
                        if script.len() > MAX_SCRIPT_BYTES {
                            let err = browser_error(
                                Some(request_id),
                                "BROWSER_INVALID_REQUEST",
                                "Script exceeds maximum size of 32 KiB",
                                false,
                                None,
                            );
                            let _ = out_tx.send(err).await;
                            return Ok(());
                        }
                    }
                    if let Some(fill_val) = p.get("text").or_else(|| p.get("value")).and_then(|v| v.as_str()) {
                        if fill_val.len() > MAX_FILL_BYTES {
                            let err = browser_error(
                                Some(request_id),
                                "BROWSER_INVALID_REQUEST",
                                "Fill text exceeds maximum size of 16 KiB",
                                false,
                                None,
                            );
                            let _ = out_tx.send(err).await;
                            return Ok(());
                        }
                    }
                }

                if !limiter.check_command(now) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_RATE_LIMITED",
                        "Command rate limit exceeded",
                        true,
                        Some(250),
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                if let Err(e) = require_permission(self.permission, DevicePermission::Control) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_FORBIDDEN",
                        e.to_string(),
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                let epoch = lease_epoch.parse::<u64>().unwrap_or(0);
                let sub_id = self.subscription_id.as_deref().unwrap_or("");
                // P1-04: verify the complete (device_id, connection_id, subscription_id, browser_id, epoch) tuple
                if !admission.broker.is_active_driver(
                    &self.device_id,
                    &self.connection_id,
                    sub_id,
                    &self.browser_id,
                    epoch,
                    now,
                ) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_DRIVER_BUSY",
                        "Not currently active driver",
                        false,
                        None,
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                let seq_num = request_seq.parse::<u64>().unwrap_or(0);
                match self.dedup.check_or_record(seq_num, now) {
                    Ok(Some(cached)) => {
                        let val: Option<serde_json::Value> = serde_json::from_slice(&cached).ok();
                        let resp = ServerMessage::BrowserResult {
                            request_id,
                            result: val,
                        };
                        let _ = out_tx.send(resp).await;
                        return Ok(());
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let err = browser_error(
                            Some(request_id),
                            "BROWSER_INVALID_REQUEST",
                            e.to_string(),
                            false,
                            None,
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                }

                // If command is navigate, validate URL scheme
                if command == "navigate" {
                    if let Some(ref p) = params {
                        if let Some(url_str) = p.get("url").and_then(|v| v.as_str()) {
                            if let Err(e) = sanitize_url(url_str) {
                                let err = browser_error(
                                    Some(request_id),
                                    "BROWSER_INVALID_REQUEST",
                                    e.to_string(),
                                    false,
                                    None,
                                );
                                let _ = out_tx.send(err).await;
                                return Ok(());
                            }
                        }
                    }
                }

                let is_eval = command == "eval";
                if is_eval && !limiter.check_eval(now) {
                    let err = browser_error(
                        Some(request_id),
                        "BROWSER_RATE_LIMITED",
                        "Eval rate limit exceeded",
                        true,
                        Some(1000),
                    );
                    let _ = out_tx.send(err).await;
                    return Ok(());
                }

                let cmd_permit = match self.command_semaphore.clone().try_acquire_owned() {
                    Ok(p) => p,
                    Err(_) => {
                        let err = browser_error(
                            Some(request_id),
                            "BROWSER_RATE_LIMITED",
                            "Command queue limit exceeded",
                            true,
                            Some(250),
                        );
                        let _ = out_tx.send(err).await;
                        return Ok(());
                    }
                };

                // P1-07: TypeSafe-approved spawned_command_tasks pattern:
                // Spawn command execution onto an async task with results returned to out_tx.
                // Reader loop is NOT blocked!
                let ctx = BrowserCommandContext {
                    browser_id: browser_id.clone(),
                    command,
                    params,
                    document_generation: Some(document_generation),
                };
                let backend = Arc::clone(backend);
                let out_tx = out_tx.clone();
                let dedup = Arc::clone(&self.dedup);
                let eval_semaphore = Arc::clone(&self.eval_semaphore);
                let cancel_token = self.cancel_token.clone();
                let broker = Arc::clone(&admission.broker);
                let device_id = self.device_id.clone();
                let connection_id = self.connection_id.clone();
                let sub_id_str = sub_id.to_string();
                let browser_id_str = self.browser_id.clone();

                tokio::spawn(async move {
                    let _held_cmd_permit = cmd_permit;

                    if cancel_token.is_cancelled() {
                        return;
                    }

                    let eval_permit = if is_eval {
                        let p = tokio::select! {
                            _ = cancel_token.cancelled() => return,
                            res = eval_semaphore.acquire() => {
                                match res {
                                    Ok(permit) => permit,
                                    Err(_) => {
                                        let err_reply = browser_error(
                                            Some(request_id),
                                            "BROWSER_EXECUTION_FAILED",
                                            "Eval dispatcher closed",
                                            false,
                                            None,
                                        );
                                        let _ = out_tx.send(err_reply).await;
                                        return;
                                    }
                                }
                            }
                        };
                        Some(p)
                    } else {
                        None
                    };

                    // Execution-time lease revalidation: RE-VALIDATE that the driver lease is still valid
                    // and has not expired or been revoked before executing! If lease expired while waiting
                    // for permit, abort with BROWSER_FORBIDDEN.
                    let now_exec = Instant::now();
                    if !broker.is_active_driver(
                        &device_id,
                        &connection_id,
                        &sub_id_str,
                        &browser_id_str,
                        epoch,
                        now_exec,
                    ) {
                        drop(eval_permit);
                        let err_reply = browser_error(
                            Some(request_id),
                            "BROWSER_FORBIDDEN",
                            "Driver lease expired or revoked before execution",
                            false,
                            None,
                        );
                        let _ = out_tx.send(err_reply).await;
                        return;
                    }

                    let res = tokio::select! {
                        _ = cancel_token.cancelled() => {
                            drop(eval_permit);
                            return;
                        }
                        r = backend.execute_command(ctx) => {
                            drop(eval_permit);
                            r
                        }
                    };

                    let reply = match res {
                        Ok(result) => {
                            let json_val = result.value.map(sanitize_json_value);
                            if let Some(ref v) = json_val {
                                if let Ok(bytes) = serde_json::to_vec(v) {
                                    dedup.record_result(seq_num, bytes, Instant::now());
                                }
                            }
                            ServerMessage::BrowserResult {
                                request_id,
                                result: json_val,
                            }
                        }
                        Err(e) => {
                            // P2-03: Preserve typed error codes from RemoteBrowserError
                            let (code, retryable, retry_after_ms) = match &e {
                                RemoteBrowserError::WaitTimeout => ("BROWSER_TIMEOUT", true, Some(1000)),
                                RemoteBrowserError::Forbidden(_) => ("BROWSER_FORBIDDEN", false, None),
                                RemoteBrowserError::Unavailable(_) => ("BROWSER_UNAVAILABLE", true, Some(2000)),
                                RemoteBrowserError::NotFound(_) => ("BROWSER_NOT_FOUND", false, None),
                                RemoteBrowserError::InvalidRequest(_) => ("BROWSER_INVALID_REQUEST", false, None),
                                RemoteBrowserError::ExecutionFailed(_) => ("BROWSER_EXECUTION_FAILED", false, None),
                            };
                            browser_error(
                                Some(request_id),
                                code,
                                e.to_string(),
                                retryable,
                                retry_after_ms,
                            )
                        }
                    };
                    let _ = out_tx.send(reply).await;
                });

                Ok(())
            }

            ClientMessage::BrowserUnsubscribe {
                request_id,
                subscription_id,
            } => {
                if let Some(backend_sub) = self.backend_subscription_id.take() {
                    let _ = backend.unsubscribe_viewer(&self.browser_id, &backend_sub).await;
                }
                self.teardown(admission);
                let resp = ServerMessage::BrowserUnsubscribed {
                    request_id,
                    subscription_id,
                };
                let _ = out_tx.send(resp).await;
                Ok(())
            }

            ClientMessage::BrowserPause { .. } => {
                self.state = WsConnectionState::Paused;
                Ok(())
            }

            ClientMessage::BrowserResume { .. } => {
                if self.subscription_id.is_some() {
                    self.state = WsConnectionState::Streaming;
                }
                Ok(())
            }
        }
    }

    pub async fn handle_client_message(
        &mut self,
        msg: ClientMessage,
        backend: &Arc<dyn RemoteBrowserBackend>,
        admission: &AdmissionController,
        now: Instant,
    ) -> Result<Option<ServerMessage>, String> {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        self.dispatch_client_message(msg, backend, admission, &tx, now).await?;
        Ok(rx.recv().await)
    }

    pub fn teardown(&mut self, admission: &AdmissionController) {
        self.cancel_token.cancel();
        let sub_id = self.subscription_id.take();
        if let Some(ref sub) = sub_id {
            admission.unsubscribe(&self.browser_id, sub);
        }
        if let Some(epoch) = self.lease_epoch.take() {
            let s_id = sub_id.as_deref().unwrap_or("");
            admission.broker.release_driver(
                &self.device_id,
                &self.connection_id,
                s_id,
                &self.browser_id,
                epoch,
            );
        }
        if let Some(queue) = self.queue.as_mut() {
            queue.close();
        }
        self.is_driver = false;
        self.state = WsConnectionState::Closed;
    }

    pub async fn teardown_with_backend(
        &mut self,
        admission: &AdmissionController,
        backend: &Arc<dyn RemoteBrowserBackend>,
    ) {
        if let Some(backend_sub) = self.backend_subscription_id.take() {
            let _ = backend.unsubscribe_viewer(&self.browser_id, &backend_sub).await;
        }
        self.teardown(admission);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::remote::browser_backend::{BrowserCommandResult, InProcessTestBackend};
    use crate::remote::browser_protocol::BrowserSubscribeOptions;

    #[test]
    fn test_ws_rejects_client_to_server_binary() {
        let mut session = BrowserWsSession::new(
            "c1".into(),
            "d1".into(),
            "b1".into(),
            DevicePermission::Control,
            Instant::now(),
        );
        let dummy_binary = vec![0x62, 1, 1, 1];
        let res = session.handle_client_binary(&dummy_binary);
        assert!(res.is_err(), "Client binary must be rejected");
    }

    #[tokio::test]
    async fn test_ws_lifecycle_subscribe_claim_command_teardown() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c1".into(),
            "d1".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: Some(70),
                interval_ms: Some(250),
                max_edge: Some(1280),
            },
        };
        let resp = session
            .handle_client_message(sub_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(resp, ServerMessage::BrowserSubscribed { .. }));
        assert_eq!(session.state, WsConnectionState::Streaming);

        // Driver claim
        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        let resp2 = session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(resp2, ServerMessage::BrowserDriverClaimed { .. }));
        assert!(session.is_driver);

        // Execute command
        let cmd_msg = ClientMessage::BrowserCommand {
            request_id: "r3".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "getState".into(),
            params: None,
        };
        let resp3 = session
            .handle_client_message(cmd_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(resp3, ServerMessage::BrowserResult { .. }));

        // Teardown
        session.teardown(&admission);
        assert_eq!(session.state, WsConnectionState::Closed);
        assert!(!session.is_driver);
    }

    #[tokio::test]
    async fn test_ws_bounded_eval_dispatcher_does_not_block_heartbeat_or_release() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c3".into(),
            "d3".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim driver
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session.handle_client_message(sub_msg, &backend, &admission, now).await.unwrap();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session.handle_client_message(claim_msg, &backend, &admission, now).await.unwrap();
        assert!(session.is_driver);

        // Heartbeat is handled immediately even if eval permit is acquired
        let _permit = session.eval_semaphore.clone().acquire_owned().await.unwrap();

        let hb_msg = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb1".into()),
            lease_epoch: session.lease_epoch.map(|e| e.to_string()),
            subscription_id: session.subscription_id.clone(),
        };
        let hb_resp = session.handle_client_message(hb_msg, &backend, &admission, now).await.unwrap().unwrap();
        assert!(matches!(hb_resp, ServerMessage::BrowserPong { .. }));

        // Driver release is also handled immediately without blocking on eval permit!
        let release_msg = ClientMessage::BrowserDriverRelease {
            request_id: "rel1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
        };
        let rel_resp = session.handle_client_message(release_msg, &backend, &admission, now).await.unwrap().unwrap();
        assert!(matches!(rel_resp, ServerMessage::BrowserDriverReleased { .. }));
        assert!(!session.is_driver);
    }

    #[tokio::test]
    async fn test_ws_view_only_permission_cannot_claim_or_command() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c2".into(),
            "d2".into(),
            "b1".into(),
            DevicePermission::View, // View only!
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r1".into(),
            subscription_id: "sub-c2".into(),
            browser_id: "b1".into(),
        };
        let resp = session
            .handle_client_message(claim_msg, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { code, .. } => {
                assert_eq!(code, "BROWSER_FORBIDDEN");
            }
            _ => panic!("Expected BROWSER_FORBIDDEN error for View-only device"),
        }
    }

    #[tokio::test]
    async fn test_ws_tuple_binding_enforcement() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-tuple".into(),
            "d-tuple".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // 1. Claim before subscribe -> rejected with BROWSER_INVALID_REQUEST
        let claim_unsub = ClientMessage::BrowserDriverClaim {
            request_id: "r-unsub".into(),
            subscription_id: "sub-fake".into(),
            browser_id: "b1".into(),
        };
        let resp = session
            .handle_client_message(claim_unsub, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Subscription binding mismatch or not subscribed"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // Subscribe properly
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session.handle_client_message(sub_msg, &backend, &admission, now).await.unwrap();

        // 2. Claim with mismatched subscription_id -> rejected
        let claim_bad_sub = ClientMessage::BrowserDriverClaim {
            request_id: "r-badsub".into(),
            subscription_id: "wrong-sub".into(),
            browser_id: "b1".into(),
        };
        let resp2 = session
            .handle_client_message(claim_bad_sub, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp2 {
            ServerMessage::BrowserError { code, .. } => assert_eq!(code, "BROWSER_INVALID_REQUEST"),
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 3. Claim with mismatched browser_id -> rejected
        let claim_bad_b = ClientMessage::BrowserDriverClaim {
            request_id: "r-badb".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b2".into(),
        };
        let resp3 = session
            .handle_client_message(claim_bad_b, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp3 {
            ServerMessage::BrowserError { code, .. } => assert_eq!(code, "BROWSER_INVALID_REQUEST"),
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 4. Command with mismatched browser_id -> rejected
        let cmd_bad_b = ClientMessage::BrowserCommand {
            request_id: "r-cmdbadb".into(),
            request_seq: "1".into(),
            browser_id: "b2".into(),
            lease_epoch: "1".into(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "getState".into(),
            params: None,
        };
        let resp4 = session
            .handle_client_message(cmd_bad_b, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp4 {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Invalid browser or subscription binding"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }
    }

    #[tokio::test]
    async fn test_ws_payload_limits_and_sanitization() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-limits".into(),
            "d-limits".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim driver
        let sub_msg = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session.handle_client_message(sub_msg, &backend, &admission, now).await.unwrap();

        let claim_msg = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session.handle_client_message(claim_msg, &backend, &admission, now).await.unwrap();

        let epoch = session.lease_epoch.unwrap().to_string();

        // 1. Oversized command name > 64 KiB
        let oversized_command = "a".repeat(65 * 1024);
        let cmd_oversized = ClientMessage::BrowserCommand {
            request_id: "r-large-cmd".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: oversized_command,
            params: None,
        };
        let resp = session
            .handle_client_message(cmd_oversized, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Command name exceeds limit"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 2. Oversized script in params > 32 KiB
        let oversized_script = "x".repeat(33 * 1024);
        let cmd_oversized_script = ClientMessage::BrowserCommand {
            request_id: "r-large-script".into(),
            request_seq: "2".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.clone(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "eval".into(),
            params: Some(serde_json::json!({ "script": oversized_script })),
        };
        let resp_script = session
            .handle_client_message(cmd_oversized_script, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp_script {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Script exceeds maximum size of 32 KiB"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }

        // 3. Oversized fill text > 16 KiB
        let oversized_fill = "y".repeat(17 * 1024);
        let cmd_oversized_fill = ClientMessage::BrowserCommand {
            request_id: "r-large-fill".into(),
            request_seq: "3".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch,
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "fill".into(),
            params: Some(serde_json::json!({ "value": oversized_fill })),
        };
        let resp_fill = session
            .handle_client_message(cmd_oversized_fill, &backend, &admission, now)
            .await
            .unwrap()
            .unwrap();
        match resp_fill {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("Fill text exceeds maximum size of 16 KiB"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST"),
        }
    }

    struct ErrorBackend {
        err: RemoteBrowserError,
    }
    impl RemoteBrowserBackend for ErrorBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>> {
            Box::pin(async move { Err(self.err.clone()) })
        }
        fn execute_command(
            &self,
            _ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
            Box::pin(async move { Err(self.err.clone()) })
        }
        fn capabilities(&self) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities> {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: false,
                    supported_formats: vec![],
                    supported_commands: vec![],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    #[tokio::test]
    async fn test_ws_typed_error_preservation() {
        let now = Instant::now();

        for (backend_err, expected_code) in [
            (RemoteBrowserError::WaitTimeout, "BROWSER_TIMEOUT"),
            (RemoteBrowserError::Forbidden("Permission denied /Users/alice".into()), "BROWSER_FORBIDDEN"),
            (RemoteBrowserError::Unavailable("GUI process exited".into()), "BROWSER_UNAVAILABLE"),
        ] {
            let admission = AdmissionController::new();
            let mut session = BrowserWsSession::new(
                "c-err".into(),
                "d-err".into(),
                "b1".into(),
                DevicePermission::Control,
                now,
            );
            let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(ErrorBackend { err: backend_err });

            let sub_msg = ClientMessage::BrowserSubscribe {
                request_id: "r1".into(),
                viewer_instance_id: "v1".into(),
                options: BrowserSubscribeOptions {
                    format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                    quality: None,
                    interval_ms: None,
                    max_edge: None,
                },
            };
            session.handle_client_message(sub_msg, &backend, &admission, now).await.unwrap();

            let claim_msg = ClientMessage::BrowserDriverClaim {
                request_id: "r2".into(),
                subscription_id: session.subscription_id.clone().unwrap(),
                browser_id: "b1".into(),
            };
            session.handle_client_message(claim_msg, &backend, &admission, now).await.unwrap();

            let cmd = ClientMessage::BrowserCommand {
                request_id: "r3".into(),
                request_seq: "1".into(),
                browser_id: "b1".into(),
                lease_epoch: session.lease_epoch.unwrap().to_string(),
                browser_instance_id: "bi1".into(),
                desktop_epoch: "1".into(),
                document_generation: "1".into(),
                command: "wait".into(),
                params: None,
            };
            let resp = session
                .handle_client_message(cmd, &backend, &admission, now)
                .await
                .unwrap()
                .unwrap();
            match resp {
                ServerMessage::BrowserError { code, message, .. } => {
                    assert_eq!(code, expected_code, "Preserve typed error code");
                    // Path redaction verification
                    assert!(!message.contains("/Users/"), "Paths must be redacted");
                }
                _ => panic!("Expected BrowserError with code {}", expected_code),
            }
        }
    }

    struct SlowBackend {
        cmd_started_tx: tokio::sync::mpsc::Sender<()>,
        cmd_continue_rx: tokio::sync::Mutex<tokio::sync::mpsc::Receiver<()>>,
    }
    impl RemoteBrowserBackend for SlowBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>> {
            Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
        }
        fn execute_command(
            &self,
            _ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
            Box::pin(async move {
                let _ = self.cmd_started_tx.send(()).await;
                let mut rx = self.cmd_continue_rx.lock().await;
                let _ = rx.recv().await;
                Ok(BrowserCommandResult {
                    success: true,
                    value: Some(serde_json::json!({ "done": true })),
                })
            })
        }
        fn capabilities(&self) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities> {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: true,
                    supported_formats: vec![],
                    supported_commands: vec![],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    #[tokio::test]
    async fn test_ws_spawned_command_tasks_non_blocking_control() {
        let (started_tx, mut started_rx) = tokio::sync::mpsc::channel(1);
        let (continue_tx, continue_rx) = tokio::sync::mpsc::channel(1);
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(SlowBackend {
            cmd_started_tx: started_tx,
            cmd_continue_rx: tokio::sync::Mutex::new(continue_rx),
        });

        let admission = AdmissionController::new();
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-nb".into(),
            "d-nb".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(32);

        // 1. Subscribe
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session.dispatch_client_message(sub, &backend, &admission, &out_tx, now).await.unwrap();
        let sub_resp = out_rx.recv().await.unwrap();
        assert!(matches!(sub_resp, ServerMessage::BrowserSubscribed { .. }));

        // 2. Driver claim
        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session.dispatch_client_message(claim, &backend, &admission, &out_tx, now).await.unwrap();
        let claim_resp = out_rx.recv().await.unwrap();
        assert!(matches!(claim_resp, ServerMessage::BrowserDriverClaimed { .. }));

        // 3. Dispatch long-running command (eval or wait)
        let cmd = ClientMessage::BrowserCommand {
            request_id: "r-long".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "wait".into(),
            params: None,
        };
        session.dispatch_client_message(cmd, &backend, &admission, &out_tx, now).await.unwrap();

        // Wait until command has actually started executing in background task
        started_rx.recv().await.unwrap();

        // 4. While command is still running in background, dispatch Heartbeat
        let hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-nb".into()),
            lease_epoch: session.lease_epoch.map(|e| e.to_string()),
            subscription_id: session.subscription_id.clone(),
        };
        session.dispatch_client_message(hb, &backend, &admission, &out_tx, now).await.unwrap();

        // Heartbeat response is received IMMEDIATELY without waiting for command!
        let hb_resp = out_rx.recv().await.unwrap();
        assert!(matches!(hb_resp, ServerMessage::BrowserPong { .. }));

        // 5. While command is still running in background, dispatch DriverRelease
        let rel = ClientMessage::BrowserDriverRelease {
            request_id: "rel-nb".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
        };
        session.dispatch_client_message(rel, &backend, &admission, &out_tx, now).await.unwrap();

        // Release response is received IMMEDIATELY!
        let rel_resp = out_rx.recv().await.unwrap();
        assert!(matches!(rel_resp, ServerMessage::BrowserDriverReleased { .. }));
        assert!(!session.is_driver);

        // 6. Now let the background command finish
        continue_tx.send(()).await.unwrap();

        // The command result arrives on out_rx
        let cmd_resp = out_rx.recv().await.unwrap();
        assert!(matches!(cmd_resp, ServerMessage::BrowserResult { .. }));
    }

    #[tokio::test]
    async fn test_ws_raw_wire_pre_parse_limit() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-raw".into(),
            "d-raw".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Raw payload over 64 KiB
        let oversized = "X".repeat(65 * 1024);
        let resp = session.handle_raw_text(&oversized, &backend, &admission, now).await.unwrap().unwrap();
        match resp {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_INVALID_REQUEST");
                assert!(message.contains("64 KiB"));
            }
            _ => panic!("Expected BROWSER_INVALID_REQUEST for oversized raw text"),
        }
    }

    struct LeakingBackend;
    impl RemoteBrowserBackend for LeakingBackend {
        fn list_sessions<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<Vec<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
            Box::pin(async move { Ok(vec![]) })
        }
        fn identify_session<'a>(
            &'a self,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<Option<crate::remote::browser_backend::RemoteBrowserSessionSummary>, RemoteBrowserError>> {
            Box::pin(async move { Ok(None) })
        }
        fn get_state<'a>(
            &'a self,
            _browser_id: &'a str,
            _scope: &'a crate::remote::browser_backend::DesktopScope,
        ) -> futures_util::future::BoxFuture<'a, Result<crate::remote::browser_backend::BrowserRemoteState, RemoteBrowserError>> {
            Box::pin(async move { Err(RemoteBrowserError::NotFound("b1".into())) })
        }
        fn execute_command(
            &self,
            _ctx: BrowserCommandContext,
        ) -> futures_util::future::BoxFuture<'_, Result<BrowserCommandResult, RemoteBrowserError>> {
            Box::pin(async move {
                Ok(BrowserCommandResult {
                    success: true,
                    value: Some(serde_json::json!({
                        "userPath": "/Users/alice/private/secret.txt",
                        "winPath": "C:\\Windows\\System32\\cmd.exe",
                        "uncPath": "\\\\nas\\share\\data.csv",
                        "safe": "normal value"
                    })),
                })
            })
        }
        fn capabilities(&self) -> futures_util::future::BoxFuture<'_, crate::remote::browser_backend::BrowserCapabilities> {
            Box::pin(async move {
                crate::remote::browser_backend::BrowserCapabilities {
                    browser_available: true,
                    supported_formats: vec![],
                    supported_commands: vec![],
                    max_edge: 2048,
                    max_fps: 8,
                }
            })
        }
    }

    #[tokio::test]
    async fn test_ws_success_response_path_sanitization() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-leak".into(),
            "d-leak".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(LeakingBackend);
        let admission = AdmissionController::new();

        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session.handle_client_message(sub, &backend, &admission, now).await.unwrap();

        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session.handle_client_message(claim, &backend, &admission, now).await.unwrap();

        let cmd = ClientMessage::BrowserCommand {
            request_id: "r3".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: session.lease_epoch.unwrap().to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "getState".into(),
            params: None,
        };
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);
        session.dispatch_client_message(cmd, &backend, &admission, &out_tx, now).await.unwrap();
        let res = out_rx.recv().await.unwrap();

        match res {
            ServerMessage::BrowserResult { result, .. } => {
                let json_str = serde_json::to_string(&result.unwrap()).unwrap();
                assert!(!json_str.contains("/Users/"), "Must redact /Users/");
                assert!(!json_str.contains("C:\\"), "Must redact C:\\");
                assert!(!json_str.contains("\\\\nas"), "Must redact UNC path");
                assert!(json_str.contains("[redacted-path]"));
                assert!(json_str.contains("normal value"));
            }
            _ => panic!("Expected BrowserResult"),
        }
    }

    #[tokio::test]
    async fn test_ws_execution_time_lease_revalidation_aborts_forbidden() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-reval".into(),
            "d-reval".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session.handle_client_message(sub, &backend, &admission, now).await.unwrap();

        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session.handle_client_message(claim, &backend, &admission, now).await.unwrap();
        let epoch = session.lease_epoch.unwrap();

        // Hold the eval semaphore so spawned eval command waits
        let held_permit = session.eval_semaphore.clone().acquire_owned().await.unwrap();

        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);
        let cmd = ClientMessage::BrowserCommand {
            request_id: "r-eval-wait".into(),
            request_seq: "1".into(),
            browser_id: "b1".into(),
            lease_epoch: epoch.to_string(),
            browser_instance_id: "bi1".into(),
            desktop_epoch: "1".into(),
            document_generation: "1".into(),
            command: "eval".into(),
            params: Some(serde_json::json!({ "script": "1+1" })),
        };
        session.dispatch_client_message(cmd, &backend, &admission, &out_tx, now).await.unwrap();

        // Now revoke driver lease via desktop reclaim while task is waiting for permit!
        admission.broker.reclaim_desktop();

        // Release the held permit, allowing the task to acquire it
        drop(held_permit);

        // Task acquires permit, re-validates lease, notices it was revoked, and aborts with BROWSER_FORBIDDEN!
        let reply = out_rx.recv().await.unwrap();
        match reply {
            ServerMessage::BrowserError { code, message, .. } => {
                assert_eq!(code, "BROWSER_FORBIDDEN");
                assert!(message.contains("expired or revoked"));
            }
            _ => panic!("Expected BROWSER_FORBIDDEN error on lease expiration"),
        }
    }

    #[tokio::test]
    async fn test_ws_heartbeat_and_release_5tuple_consistency() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-5t".into(),
            "d-5t".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let admission = AdmissionController::new();

        // Subscribe & claim
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r1".into(),
            viewer_instance_id: "v1".into(),
            options: BrowserSubscribeOptions {
                format: crate::remote::browser_protocol::BrowserImageFormat::Jpeg,
                quality: None,
                interval_ms: None,
                max_edge: None,
            },
        };
        session.handle_client_message(sub, &backend, &admission, now).await.unwrap();

        let claim = ClientMessage::BrowserDriverClaim {
            request_id: "r2".into(),
            subscription_id: session.subscription_id.clone().unwrap(),
            browser_id: "b1".into(),
        };
        session.handle_client_message(claim, &backend, &admission, now).await.unwrap();
        let epoch = session.lease_epoch.unwrap();

        // 1. Heartbeat with wrong subscription_id -> BROWSER_INVALID_REQUEST
        let bad_sub_hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-bad-sub".into()),
            lease_epoch: Some(epoch.to_string()),
            subscription_id: Some("sub-wrong".into()),
        };
        let bad_hb_resp = session.handle_client_message(bad_sub_hb, &backend, &admission, now).await.unwrap().unwrap();
        assert!(matches!(bad_hb_resp, ServerMessage::BrowserError { ref code, .. } if code == "BROWSER_INVALID_REQUEST"));

        // 2. Heartbeat with wrong lease_epoch -> BROWSER_INVALID_REQUEST
        let bad_epoch_hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-bad-epoch".into()),
            lease_epoch: Some("99999".into()),
            subscription_id: session.subscription_id.clone(),
        };
        let bad_epoch_resp = session.handle_client_message(bad_epoch_hb, &backend, &admission, now).await.unwrap().unwrap();
        assert!(matches!(bad_epoch_resp, ServerMessage::BrowserError { ref code, .. } if code == "BROWSER_INVALID_REQUEST"));

        // 3. Heartbeat with correct 5-tuple -> BrowserPong
        let good_hb = ClientMessage::BrowserHeartbeat {
            request_id: Some("hb-good".into()),
            lease_epoch: Some(epoch.to_string()),
            subscription_id: session.subscription_id.clone(),
        };
        let good_hb_resp = session.handle_client_message(good_hb, &backend, &admission, now).await.unwrap().unwrap();
        assert!(matches!(good_hb_resp, ServerMessage::BrowserPong { .. }));

        // 4. Release with wrong lease_epoch -> BROWSER_INVALID_REQUEST
        let bad_rel = ClientMessage::BrowserDriverRelease {
            request_id: "rel-bad".into(),
            lease_epoch: "99999".into(),
        };
        let bad_rel_resp = session.handle_client_message(bad_rel, &backend, &admission, now).await.unwrap().unwrap();
        assert!(matches!(bad_rel_resp, ServerMessage::BrowserError { ref code, .. } if code == "BROWSER_INVALID_REQUEST"));

        // 5. Release with correct 5-tuple -> BrowserDriverReleased
        let good_rel = ClientMessage::BrowserDriverRelease {
            request_id: "rel-good".into(),
            lease_epoch: epoch.to_string(),
        };
        let good_rel_resp = session.handle_client_message(good_rel, &backend, &admission, now).await.unwrap().unwrap();
        assert!(matches!(good_rel_resp, ServerMessage::BrowserDriverReleased { .. }));
        assert!(!session.is_driver);
    }

    #[test]
    fn test_ws_frame_enqueuing_and_ack_flow_control() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-flow".into(),
            "d-flow".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );

        // Before subscribing: frames are dropped
        assert!(session.enqueue_frame(vec![0x62, 1, 1, 1, 1, 0, 0, 0], now).is_none());

        // Setup queue (as if subscribed)
        session.queue = Some(SubscriberQueue::new("sub-flow".into(), 1));

        // Frame 1: admitted immediately
        let f1 = vec![0x62, 1, 1, 1, 1, 0, 0, 0, 10, 20];
        let res1 = session.enqueue_frame(f1.clone(), now);
        assert_eq!(res1, Some(f1));

        // Frame 2: unacked frame exists -> backpressured into pending slot
        let f2 = vec![0x62, 1, 1, 1, 2, 0, 0, 0, 30, 40];
        assert!(session.enqueue_frame(f2.clone(), now).is_none());

        // Frame 3: overwrites pending slot (latest-only!)
        let f3 = vec![0x62, 1, 1, 1, 3, 0, 0, 0, 50, 60];
        assert!(session.enqueue_frame(f3.clone(), now).is_none());

        // ACK arrives for frame 1: frame 3 is promoted and stored in pending_promoted_frame
        let ack = ClientMessage::BrowserFrameAck { stream_id: 1, seq: 1 };
        let admission = AdmissionController::new();
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let (out_tx, _) = tokio::sync::mpsc::channel(16);
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        rt.block_on(session.dispatch_client_message(ack, &backend, &admission, &out_tx, now)).unwrap();

        assert_eq!(session.pending_promoted_frame, Some(f3));
    }

    #[tokio::test]
    async fn test_ws_r10_pause_and_resume_protocol() {
        let now = Instant::now();
        let mut session = BrowserWsSession::new(
            "c-pause".into(),
            "d-pause".into(),
            "b1".into(),
            DevicePermission::Control,
            now,
        );

        let admission = AdmissionController::new();
        let backend: Arc<dyn RemoteBrowserBackend> = Arc::new(InProcessTestBackend::new());
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(16);

        // 1. Subscribe to enter Streaming state
        let sub = ClientMessage::BrowserSubscribe {
            request_id: "r-sub".into(),
            viewer_instance_id: "v1".into(),
            options: Default::default(),
        };
        session.dispatch_client_message(sub, &backend, &admission, &out_tx, now).await.unwrap();
        let _ = out_rx.recv().await.unwrap(); // BrowserSubscribed
        assert_eq!(session.state, WsConnectionState::Streaming);

        // Frame 1 admitted when streaming
        let f1 = vec![0x62, 1, 1, 1, 1, 0, 0, 0, 10, 20];
        assert_eq!(session.enqueue_frame(f1.clone(), now), Some(f1));

        // Acknowledge frame 1 so queue has no unacked frame
        let ack1 = ClientMessage::BrowserFrameAck { stream_id: 1, seq: 1 };
        session.dispatch_client_message(ack1, &backend, &admission, &out_tx, now).await.unwrap();

        // 2. Pause session via BrowserPause
        let pause = ClientMessage::BrowserPause {
            request_id: Some("p1".into()),
            subscription_id: session.subscription_id.clone(),
        };
        session.dispatch_client_message(pause, &backend, &admission, &out_tx, now).await.unwrap();
        assert_eq!(session.state, WsConnectionState::Paused);
        assert!(session.subscription_id.is_some(), "Session not torn down on pause");

        // While paused: frames are NOT forwarded (enqueue_frame returns None even without unacked frame)
        let f2 = vec![0x62, 1, 1, 1, 2, 0, 0, 0, 30, 40];
        assert_eq!(session.enqueue_frame(f2.clone(), now), None);

        // While paused: ACKs are paused and do NOT promote frames
        let ack2 = ClientMessage::BrowserFrameAck { stream_id: 1, seq: 2 };
        session.dispatch_client_message(ack2, &backend, &admission, &out_tx, now).await.unwrap();
        assert_eq!(session.pending_promoted_frame, None);

        // 3. Resume session via BrowserResume
        let resume = ClientMessage::BrowserResume {
            request_id: Some("r1".into()),
            subscription_id: session.subscription_id.clone(),
        };
        session.dispatch_client_message(resume, &backend, &admission, &out_tx, now).await.unwrap();
        assert_eq!(session.state, WsConnectionState::Streaming);

        // After resume: frame forwarding works again
        let f3 = vec![0x62, 1, 1, 1, 3, 0, 0, 0, 50, 60];
        assert_eq!(session.enqueue_frame(f3.clone(), now), Some(f3));

        // Subsequent frame is backpressured until ACK
        let f4 = vec![0x62, 1, 1, 1, 4, 0, 0, 0, 70, 80];
        assert_eq!(session.enqueue_frame(f4.clone(), now), None);

        // ACK for frame 3 promotes frame 4
        let ack3 = ClientMessage::BrowserFrameAck { stream_id: 1, seq: 3 };
        session.dispatch_client_message(ack3, &backend, &admission, &out_tx, now).await.unwrap();
        assert_eq!(session.pending_promoted_frame, Some(f4));
    }
}
