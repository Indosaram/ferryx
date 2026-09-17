//! Remote Browser WebSocket Per-Connection Lifetime, Actors & Dispatcher
//! Authoritative Spec: docs/plans/REMOTE_BROWSER_SCREENCAST_PLAN_2026-09-17.md (§4.3, §4.4, §5, §6.2)

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use crate::remote::auth::DevicePermission;
use crate::remote::browser_admission::{AdmissionController, SubscriberQueue};
use crate::remote::browser_backend::{
    BrowserCommandContext, BrowserCommandResult, RemoteBrowserBackend, RemoteBrowserError,
};
use crate::remote::browser_protocol::{ClientMessage, ServerMessage};
use crate::remote::browser_security::{
    require_permission, sanitize_url, RequestDeduplicator,
};

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
    pub dedup: RequestDeduplicator,
    pub eval_semaphore: Arc<Semaphore>,
    pub last_heartbeat: Instant,
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
            dedup: RequestDeduplicator::new(),
            eval_semaphore: Arc::new(Semaphore::new(1)),
            last_heartbeat: now,
        }
    }

    pub fn handle_client_binary(&mut self, _bytes: &[u8]) -> Result<(), String> {
        // §4.2: Client to server binary frames are strictly rejected
        Err("Client-to-server binary WebSocket frames are rejected".into())
    }

    pub async fn handle_client_message(
        &mut self,
        msg: ClientMessage,
        backend: &dyn RemoteBrowserBackend,
        admission: &AdmissionController,
        now: Instant,
    ) -> Result<Option<ServerMessage>, String> {
        let limiter = admission.get_device_limiter(&self.device_id, now);

        match msg {
            ClientMessage::BrowserSubscribe {
                request_id,
                viewer_instance_id,
                options,
            } => {
                let sub_id = format!("sub-{}", self.connection_id);
                admission
                    .try_subscribe(
                        &self.browser_id,
                        &sub_id,
                        &self.device_id,
                        &viewer_instance_id,
                    )
                    .map_err(|e| format!("Subscription failed: {}", e))?;

                let queue = SubscriberQueue::new(sub_id.clone(), self.stream_id);
                self.queue = Some(queue);
                self.subscription_id = Some(sub_id.clone());
                self.state = WsConnectionState::Streaming;

                Ok(Some(ServerMessage::BrowserSubscribed {
                    request_id,
                    subscription_id: sub_id,
                    stream_id: self.stream_id,
                    browser_id: self.browser_id.clone(),
                    browser_instance_id: "bi1".into(),
                    browser_service_epoch: "1".into(),
                    desktop_epoch: "1".into(),
                    document_generation: "1".into(),
                    options,
                }))
            }

            ClientMessage::BrowserFrameAck { stream_id, seq } => {
                if let Some(queue) = self.queue.as_mut() {
                    let _promoted = queue.acknowledge_frame(stream_id, seq, now);
                }
                Ok(None)
            }

            ClientMessage::BrowserHeartbeat {
                request_id,
                lease_epoch,
                subscription_id: _,
            } => {
                self.last_heartbeat = now;
                if let Some(epoch_str) = lease_epoch {
                    if let Ok(epoch) = epoch_str.parse::<u64>() {
                        let _ = admission.broker.refresh_lease(&self.device_id, epoch, now);
                    }
                }
                Ok(Some(ServerMessage::BrowserPong {
                    request_id,
                    timestamp: Some(now.elapsed().as_secs_f64()),
                }))
            }

            ClientMessage::BrowserDriverClaim {
                request_id,
                subscription_id,
                browser_id,
            } => {
                if !limiter.check_claim(now) {
                    return Ok(Some(ServerMessage::BrowserError {
                        request_id: Some(request_id),
                        code: "BROWSER_RATE_LIMITED".into(),
                        message: "Driver claim rate limit exceeded".into(),
                        retryable: true,
                        retry_after_ms: Some(500),
                    }));
                }

                if let Err(e) = require_permission(self.permission, DevicePermission::Control) {
                    return Ok(Some(ServerMessage::BrowserError {
                        request_id: Some(request_id),
                        code: "BROWSER_FORBIDDEN".into(),
                        message: e.to_string(),
                        retryable: false,
                        retry_after_ms: None,
                    }));
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
                        Ok(Some(ServerMessage::BrowserDriverClaimed {
                            request_id,
                            lease_epoch: lease.lease_epoch.to_string(),
                            expires_at: lease.expires_at.elapsed().as_secs_f64(),
                        }))
                    }
                    Err(code) => Ok(Some(ServerMessage::BrowserError {
                        request_id: Some(request_id),
                        code,
                        message: "Another remote driver is active".into(),
                        retryable: true,
                        retry_after_ms: Some(1000),
                    })),
                }
            }

            ClientMessage::BrowserDriverRelease {
                request_id,
                lease_epoch,
            } => {
                let epoch = lease_epoch.parse::<u64>().unwrap_or(0);
                admission.broker.release_driver(&self.device_id, epoch);
                self.is_driver = false;
                self.lease_epoch = None;
                Ok(Some(ServerMessage::BrowserDriverReleased {
                    request_id,
                    lease_epoch: Some(lease_epoch),
                }))
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
                if !limiter.check_command(now) {
                    return Ok(Some(ServerMessage::BrowserError {
                        request_id: Some(request_id),
                        code: "BROWSER_RATE_LIMITED".into(),
                        message: "Command rate limit exceeded".into(),
                        retryable: true,
                        retry_after_ms: Some(250),
                    }));
                }

                if let Err(e) = require_permission(self.permission, DevicePermission::Control) {
                    return Ok(Some(ServerMessage::BrowserError {
                        request_id: Some(request_id),
                        code: "BROWSER_FORBIDDEN".into(),
                        message: e.to_string(),
                        retryable: false,
                        retry_after_ms: None,
                    }));
                }

                let epoch = lease_epoch.parse::<u64>().unwrap_or(0);
                if !admission
                    .broker
                    .is_active_driver(&self.device_id, epoch, now)
                {
                    return Ok(Some(ServerMessage::BrowserError {
                        request_id: Some(request_id),
                        code: "BROWSER_DRIVER_BUSY".into(),
                        message: "Not currently active driver".into(),
                        retryable: false,
                        retry_after_ms: None,
                    }));
                }

                let seq_num = request_seq.parse::<u64>().unwrap_or(0);
                if let Some(cached) = self
                    .dedup
                    .check_or_record(seq_num, now)
                    .map_err(|e| e.to_string())?
                {
                    let val: Option<serde_json::Value> = serde_json::from_slice(&cached).ok();
                    return Ok(Some(ServerMessage::BrowserResult {
                        request_id,
                        result: val,
                    }));
                }

                // If command is navigate, validate URL scheme
                if command == "navigate" {
                    if let Some(ref p) = params {
                        if let Some(url_str) = p.get("url").and_then(|v| v.as_str()) {
                            if let Err(e) = sanitize_url(url_str) {
                                return Ok(Some(ServerMessage::BrowserError {
                                    request_id: Some(request_id),
                                    code: "BROWSER_INVALID_REQUEST".into(),
                                    message: e.to_string(),
                                    retryable: false,
                                    retry_after_ms: None,
                                }));
                            }
                        }
                    }
                }

                // Bounded dispatcher for eval to avoid blocking
                let ctx = BrowserCommandContext {
                    browser_id,
                    command: command.clone(),
                    params,
                    document_generation: Some(document_generation),
                };

                let res: Result<BrowserCommandResult, RemoteBrowserError> = if command == "eval" {
                    if !limiter.check_eval(now) {
                        return Ok(Some(ServerMessage::BrowserError {
                            request_id: Some(request_id),
                            code: "BROWSER_RATE_LIMITED".into(),
                            message: "Eval rate limit exceeded".into(),
                            retryable: true,
                            retry_after_ms: Some(1000),
                        }));
                    }
                    let _permit = self
                        .eval_semaphore
                        .acquire()
                        .await
                        .map_err(|_| "Eval dispatcher closed")?;
                    backend.execute_command(ctx).await
                } else {
                    backend.execute_command(ctx).await
                };

                match res {
                    Ok(result) => {
                        let json_val = result.value.clone();
                        if let Some(ref v) = json_val {
                            if let Ok(bytes) = serde_json::to_vec(v) {
                                self.dedup.record_result(seq_num, bytes, now);
                            }
                        }
                        Ok(Some(ServerMessage::BrowserResult {
                            request_id,
                            result: json_val,
                        }))
                    }
                    Err(e) => Ok(Some(ServerMessage::BrowserError {
                        request_id: Some(request_id),
                        code: "BROWSER_EXECUTION_FAILED".into(),
                        message: e.to_string(),
                        retryable: false,
                        retry_after_ms: None,
                    })),
                }
            }

            ClientMessage::BrowserUnsubscribe {
                request_id,
                subscription_id,
            } => {
                self.teardown(admission);
                Ok(Some(ServerMessage::BrowserUnsubscribed {
                    request_id,
                    subscription_id,
                }))
            }
        }
    }

    pub fn teardown(&mut self, admission: &AdmissionController) {
        if let Some(sub_id) = self.subscription_id.take() {
            admission.unsubscribe(&self.browser_id, &sub_id);
        }
        if let Some(epoch) = self.lease_epoch.take() {
            admission.broker.release_driver(&self.device_id, epoch);
        }
        if let Some(queue) = self.queue.as_mut() {
            queue.close();
        }
        self.is_driver = false;
        self.state = WsConnectionState::Closed;
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::remote::browser_backend::InProcessTestBackend;
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
        let backend = InProcessTestBackend::new();
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
        let backend = InProcessTestBackend::new();
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
        let backend = InProcessTestBackend::new();
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
}
