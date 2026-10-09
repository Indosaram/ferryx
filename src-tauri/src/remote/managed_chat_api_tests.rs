#[cfg(test)]
mod tests {
    use crate::worktree::WorkspaceRegistry;
    use crate::remote::{
        auth::{DeviceAccessScope, DevicePermission},
        managed_chat_api::{
            managed_chat_router, register_managed_provider, MANAGED_PROVIDERS,
            CallbackKind, CallbackOption, CallbackQuestion,
            CallbackStatus, LiveCallbackEntry, ManagedChatProvider, LIVE_CALLBACKS,
        },
        state::RemoteGatewayState,
    };
    use crate::scoped_contracts::{
        AttachmentMediaType, AttachmentReceipt, DeliveryReceipt, DeliveryStage, Epoch, TargetRef,
        ATTACHMENT_MAX_FILE_BYTES, ATTACHMENT_MAX_FILES_PER_TURN, ATTACHMENT_MAX_TURN_BYTES,
    };
    use axum::http::StatusCode;
    use futures_util::future::BoxFuture;
    use parking_lot::Mutex;
    use serde_json::{json, Value};
    use std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{atomic::Ordering, Arc},
        time::Instant,
    };

    #[derive(Default)]
    struct TestSessionBackend {
        sessions: Mutex<HashMap<String, crate::remote::backend::RemoteSessionDetails>>,
    }

    impl crate::remote::backend::RemoteSessionBackend for TestSessionBackend {
        fn list_sessions(
            &self,
        ) -> BoxFuture<'_, Vec<String>> {
            let list = self.sessions.lock().keys().cloned().collect();
            Box::pin(async move { list })
        }
        fn describe_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> BoxFuture<'a, Result<crate::remote::backend::RemoteSessionDetails, String>> {
            let res = self
                .sessions
                .lock()
                .get(session_id)
                .cloned()
                .ok_or_else(|| "not found".to_string());
            Box::pin(async move { res })
        }
        fn attach_with_sequence<'a>(
            &'a self,
            _session_id: &'a str,
            _after_seq: Option<u64>,
        ) -> BoxFuture<'a, Result<crate::terminal::SessionAttachment, String>> {
            Box::pin(async move { Err("unimplemented".into()) })
        }
        fn write_input<'a>(
            &'a self,
            _session_id: &'a str,
            _data: &'a [u8],
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async move { Ok(()) })
        }
        fn resize<'a>(
            &'a self,
            _session_id: &'a str,
            _cols: u16,
            _rows: u16,
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async move { Ok(()) })
        }
        fn signal<'a>(&'a self, _session_id: &'a str, _signal: crate::terminal::TerminalSignal) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async { Ok(()) })
        }
    }

    struct MockProvider {
        sent_inputs: Mutex<Vec<Vec<Value>>>,
        replied_callbacks: Mutex<Vec<(Value, String, String, Value)>>,
        stopped: Mutex<bool>,
        fail_next_reply: Mutex<bool>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                sent_inputs: Mutex::new(Vec::new()),
                replied_callbacks: Mutex::new(Vec::new()),
                stopped: Mutex::new(false),
                fail_next_reply: Mutex::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl ManagedChatProvider for MockProvider {
        async fn send_turn(
            &self,
            target: &TargetRef,
            input: Vec<Value>,
        ) -> Result<DeliveryReceipt, String> {
            self.sent_inputs.lock().push(input);
            Ok(DeliveryReceipt {
                request_id: uuid::Uuid::new_v4().to_string(),
                target: target.clone(),
                stage: DeliveryStage::Accepted,
            })
        }

        async fn reply_callback(
            &self,
            callback_id: Value,
            thread_id: &str,
            turn_id: &str,
            result: Value,
        ) -> Result<(), String> {
            if std::mem::take(&mut *self.fail_next_reply.lock()) {
                return Err("injected dispatch failure".into());
            }
            self.replied_callbacks.lock().push((
                callback_id,
                thread_id.to_string(),
                turn_id.to_string(),
                result,
            ));
            Ok(())
        }

        async fn stop_agent(&self, _target: &TargetRef) -> Result<(), String> {
            *self.stopped.lock() = true;
            Ok(())
        }
    }

    fn setup_test_server(
        backend: Arc<TestSessionBackend>,
    ) -> (
        Arc<RemoteGatewayState>,
        String,
        String,
        SocketAddr,
        tokio::sync::oneshot::Sender<()>,
        tokio::task::JoinHandle<()>,
    ) {
        for id in backend.sessions.lock().keys() {
            if id != "sess-noprov" && id != "sess-no-provider" {
                register_managed_provider(id, Arc::new(MockProvider::new()));
            }
        }
        let state = Arc::new(RemoteGatewayState::new_with_backend(
            backend,
            WorkspaceRegistry::new(),
        ));
        state.daemon_epoch.store(1, Ordering::SeqCst);

        let pin = state
            .auth_manager
            .create_scoped_pairing_code(DevicePermission::Control, DeviceAccessScope::Machine)
            .unwrap();
        let (token, device) = state
            .auth_manager
            .exchange_pairing_code(&pin, "test-control-device")
            .unwrap();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let tokio_listener = tokio::net::TcpListener::from_std(listener).unwrap();

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let app = managed_chat_router(Arc::clone(&state));

        let handle = tokio::spawn(async move {
            let _ = axum::serve(tokio_listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stop_rx.await;
                })
                .await;
        });

        (state, token, device.id, addr, stop_tx, handle)
    }

    #[tokio::test]
    async fn test_reply_approval_live_success_and_replay_failure() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-1".to_string(),
            crate::remote::backend::RemoteSessionDetails {
                session_id: "sess-1".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let target = TargetRef {
            host_id: "host-local".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-1".to_string(),
        };

        {
            let mut reg = LIVE_CALLBACKS.lock();
            reg.register(LiveCallbackEntry {
                callback_id: "cb-approval-1".to_string(),
                thread_id: "thread-1".to_string(),
                turn_id: "turn-1".to_string(),
                callback_incarnation: 0,
                target: target.clone(),
                kind: CallbackKind::Approval,
                text: Some("Execute cargo check?".to_string()),
                questions: None,
                status: CallbackStatus::Pending,
                created_at: Instant::now(),
            }).unwrap();
        }
        let incarnation = LIVE_CALLBACKS.lock().get("sess-1", "cb-approval-1").unwrap().callback_incarnation;

        let reply_url = format!("http://{}/api/v1/chat/reply", addr);
        let body = json!({
            "requestId": "req-reply-1",
            "target": target,
            "callbackId": "cb-approval-1",
            "threadId": "thread-1",
            "turnId": "turn-1",
            "callbackIncarnation": incarnation,
            "result": { "decision": "accept" }
        });

        let resp = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let resp_json: Value = resp.json().await.unwrap();
        assert_eq!(resp_json["ok"], true);
        assert_eq!(resp_json["requestId"], "req-reply-1");
        assert_eq!(resp_json["data"]["resolved"], true);
        assert_eq!(resp_json["data"]["callbackId"], "cb-approval-1");

        let replay_resp = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap();

        assert_eq!(replay_resp.status(), StatusCode::CONFLICT);
        let replay_json: Value = replay_resp.json().await.unwrap();
        assert_eq!(replay_json["ok"], false);
        assert_eq!(replay_json["error"]["code"], "REQUEST_CONFLICT");

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_reply_question_live_success_and_discovery_route() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-q".to_string(),
            crate::remote::backend::RemoteSessionDetails {
                session_id: "sess-q".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let target = TargetRef {
            host_id: "host-local".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-q".to_string(),
        };

        {
            let mut reg = LIVE_CALLBACKS.lock();
            reg.register(LiveCallbackEntry {
                callback_id: "cb-q-1".to_string(),
                thread_id: "thread-q".to_string(),
                turn_id: "turn-q1".to_string(),
                callback_incarnation: 0,
                target: target.clone(),
                kind: CallbackKind::Question,
                text: None,
                questions: Some(vec![CallbackQuestion {
                    id: "target_arch".to_string(),
                    question: "Target architecture?".to_string(),
                    is_secret: Some(false),
                    options: Some(vec![CallbackOption {
                        label: "arm64".to_string(),
                        description: "ARM 64-bit".to_string(),
                    }]),
                }]),
                status: CallbackStatus::Pending,
                created_at: Instant::now(),
            }).unwrap();
        }
        let incarnation = LIVE_CALLBACKS.lock().get("sess-q", "cb-q-1").unwrap().callback_incarnation;

        let callbacks_url = format!(
            "http://{}/api/v1/chat/callbacks?backendSessionId=sess-q&threadId=thread-q",
            addr
        );
        let disc_resp = client
            .get(&callbacks_url)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(disc_resp.status(), StatusCode::OK);
        let disc_json: Value = disc_resp.json().await.unwrap();
        assert_eq!(disc_json["ok"], true);
        let items = disc_json["data"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["callbackId"], "cb-q-1");
        assert_eq!(items[0]["callbackIncarnation"], incarnation);
        assert_eq!(items[0]["target"]["hostId"], "host-local");
        assert_eq!(items[0]["target"]["ownerId"], device_id.as_str());
        assert_eq!(items[0]["target"]["epoch"], "1");
        assert_eq!(items[0]["target"]["backendSessionId"], "sess-q");
        assert_eq!(items[0]["questions"][0]["id"], "target_arch");

        let reply_url = format!("http://{}/api/v1/chat/reply", addr);

        let invalid_body = json!({
            "requestId": "req-q-bad",
            "target": target,
            "callbackId": "cb-q-1",
            "threadId": "thread-q",
            "turnId": "turn-q1",
            "callbackIncarnation": incarnation,
            "result": { "answers": {} }
        });

        let bad_resp = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&invalid_body)
            .send()
            .await
            .unwrap();

        assert_eq!(bad_resp.status(), StatusCode::BAD_REQUEST);

        let valid_body = json!({
            "requestId": "req-q-ok",
            "target": target,
            "callbackId": "cb-q-1",
            "threadId": "thread-q",
            "turnId": "turn-q1",
            "callbackIncarnation": incarnation,
            "result": {
                "answers": {
                    "target_arch": {
                        "answers": ["arm64"]
                    }
                }
            }
        });

        let ok_resp = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&valid_body)
            .send()
            .await
            .unwrap();

        assert_eq!(ok_resp.status(), StatusCode::OK);
        let ok_json: Value = ok_resp.json().await.unwrap();
        assert_eq!(ok_json["ok"], true);
        assert_eq!(ok_json["data"]["resolved"], true);

        let disc_resp2 = client
            .get(&callbacks_url)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(disc_resp2.status(), StatusCode::OK);
        let disc_json2: Value = disc_resp2.json().await.unwrap();
        let items2 = disc_json2["data"].as_array().unwrap();
        assert_eq!(items2.len(), 0);

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_reply_transcript_prose_fails_closed() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-prose".to_string(),
            crate::remote::backend::RemoteSessionDetails {
                session_id: "sess-prose".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let target = TargetRef {
            host_id: "host-local".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-prose".to_string(),
        };

        let reply_url = format!("http://{}/api/v1/chat/reply", addr);
        let body = json!({
            "requestId": "req-prose",
            "target": target,
            "callbackId": "unregistered-transcript-callback-id",
            "threadId": "thread-1",
            "turnId": "turn-1",
            "callbackIncarnation": 1,
            "result": { "decision": "accept" }
        });

        let resp = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp_json: Value = resp.json().await.unwrap();
        assert_eq!(resp_json["ok"], false);
        assert_eq!(resp_json["error"]["code"], "NOT_FOUND");

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_reply_same_id_same_turn_replacement_rejects_stale_incarnation() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-replace".to_string(), crate::remote::backend::RemoteSessionDetails {
            session_id: "sess-replace".to_string(), cols: 80, rows: 24, workspace_id: None,
            worktree_label: None, worktree_path: None, running: true,
        });
        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let target = TargetRef { host_id: "host-local".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-replace".into() };
        let old_incarnation = LIVE_CALLBACKS.lock().register(LiveCallbackEntry {
            callback_id: "reused-id".into(), thread_id: "same-thread".into(), turn_id: "same-turn".into(),
            callback_incarnation: 0, target: target.clone(), kind: CallbackKind::Approval, text: None,
            questions: None, status: CallbackStatus::Pending, created_at: Instant::now(),
        }).unwrap();
        let new_incarnation = LIVE_CALLBACKS.lock().register(LiveCallbackEntry {
            callback_id: "reused-id".into(), thread_id: "same-thread".into(), turn_id: "same-turn".into(),
            callback_incarnation: 0, target: target.clone(), kind: CallbackKind::Approval, text: None,
            questions: None, status: CallbackStatus::Pending, created_at: Instant::now(),
        }).unwrap();
        assert!(new_incarnation > old_incarnation);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{}/api/v1/chat/reply", addr);
        let make_body = |request_id: &str, incarnation| json!({
            "requestId": request_id, "target": target, "callbackId": "reused-id",
            "threadId": "same-thread", "turnId": "same-turn", "callbackIncarnation": incarnation,
            "result": { "decision": "accept" }
        });
        let stale = client.post(&url).bearer_auth(&token).json(&make_body("stale", old_incarnation)).send().await.unwrap();
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        let stale_json: Value = stale.json().await.unwrap();
        assert_eq!(stale_json["error"]["code"], "STALE_CALLBACK");
        assert_eq!(LIVE_CALLBACKS.lock().get("sess-replace", "reused-id").unwrap().status, CallbackStatus::Pending);
        let current = client.post(&url).bearer_auth(&token).json(&make_body("current", new_incarnation)).send().await.unwrap();
        assert_eq!(current.status(), StatusCode::OK);
        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_reply_dispatch_failure_restores_pending_for_retry() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-retry".to_string(), crate::remote::backend::RemoteSessionDetails {
            session_id: "sess-retry".to_string(), cols: 80, rows: 24, workspace_id: None,
            worktree_label: None, worktree_path: None, running: true,
        });
        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let provider = Arc::new(MockProvider::new());
        *provider.fail_next_reply.lock() = true;
        register_managed_provider("sess-retry", provider);
        let target = TargetRef { host_id: "host-local".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-retry".into() };
        let incarnation = LIVE_CALLBACKS.lock().register(LiveCallbackEntry {
            callback_id: "retry-id".into(), thread_id: "thread".into(), turn_id: "turn".into(),
            callback_incarnation: 0, target: target.clone(), kind: CallbackKind::Approval, text: None,
            questions: None, status: CallbackStatus::Pending, created_at: Instant::now(),
        }).unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{}/api/v1/chat/reply", addr);
        let make_body = |request_id: &str| json!({ "requestId": request_id, "target": target,
            "callbackId": "retry-id", "threadId": "thread", "turnId": "turn",
            "callbackIncarnation": incarnation, "result": { "decision": "accept" } });
        let failed = client.post(&url).bearer_auth(&token).json(&make_body("failed")).send().await.unwrap();
        assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let failed_json: Value = failed.json().await.unwrap();
        assert_eq!(failed_json["error"]["code"], "UNSUPPORTED");
        assert!(failed_json["error"]["message"].as_str().unwrap().contains("PROVIDER_DISPATCH_FAILED"));
        assert_eq!(LIVE_CALLBACKS.lock().get("sess-retry", "retry-id").unwrap().status, CallbackStatus::Pending);
        let retried = client.post(&url).bearer_auth(&token).json(&make_body("retry")).send().await.unwrap();
        assert_eq!(retried.status(), StatusCode::OK);
        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_reply_without_provider_does_not_resolve_callback() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-no-provider".to_string(), crate::remote::backend::RemoteSessionDetails {
            session_id: "sess-no-provider".to_string(), cols: 80, rows: 24, workspace_id: None,
            worktree_label: None, worktree_path: None, running: true,
        });
        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let target = TargetRef { host_id: "host-local".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-no-provider".into() };
        let incarnation = LIVE_CALLBACKS.lock().register(LiveCallbackEntry {
            callback_id: "provider-gone".into(), thread_id: "thread".into(), turn_id: "turn".into(),
            callback_incarnation: 0, target: target.clone(), kind: CallbackKind::Approval, text: None,
            questions: None, status: CallbackStatus::Pending, created_at: Instant::now(),
        }).unwrap();
        MANAGED_PROVIDERS.lock().remove("sess-no-provider");
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{}/api/v1/chat/reply", addr);
        let body = json!({ "requestId": "no-provider", "target": target, "callbackId": "provider-gone",
            "threadId": "thread", "turnId": "turn", "callbackIncarnation": incarnation,
            "result": { "decision": "accept" } });
        let response = client.post(&url).bearer_auth(&token).json(&body).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let response_json: Value = response.json().await.unwrap();
        assert_eq!(response_json["error"]["code"], "UNSUPPORTED");
        assert_eq!(LIVE_CALLBACKS.lock().get("sess-no-provider", "provider-gone").unwrap().status, CallbackStatus::Pending);
        register_managed_provider("sess-no-provider", Arc::new(MockProvider::new()));
        let retry = client.post(&url).bearer_auth(&token).json(&body).send().await.unwrap();
        assert_eq!(retry.status(), StatusCode::CONFLICT);
        let retry_json: Value = retry.json().await.unwrap();
        assert_eq!(retry_json["error"]["code"], "TARGET_EXPIRED");
        assert!(matches!(
            &LIVE_CALLBACKS.lock().get("sess-no-provider", "provider-gone").unwrap().status,
            CallbackStatus::Invalidated { reason, .. } if reason == "PROVIDER_REPLACED"
        ));
        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_reply_owner_mismatch_and_turn_advancement() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-guard".to_string(),
            crate::remote::backend::RemoteSessionDetails {
                session_id: "sess-guard".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let target = TargetRef {
            host_id: "host-local".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-guard".to_string(),
        };

        let mut foreign_owner_target = target.clone();
        foreign_owner_target.owner_id = "foreign-device-id".to_string();

        let reply_url = format!("http://{}/api/v1/chat/reply", addr);
        let r_owner = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&json!({
                "requestId": "req-owner-test",
                "target": foreign_owner_target,
                "callbackId": "cb-1",
                "threadId": "th-1",
                "turnId": "turn-1",
                "callbackIncarnation": 1,
                "result": { "decision": "accept" }
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(r_owner.status(), StatusCode::FORBIDDEN);

        {
            let mut reg = LIVE_CALLBACKS.lock();
            reg.register(LiveCallbackEntry {
                callback_id: "cb-turn-1".to_string(),
                thread_id: "th-1".to_string(),
                turn_id: "turn-1".to_string(),
                callback_incarnation: 0,
                target: target.clone(),
                kind: CallbackKind::Approval,
                text: None,
                questions: None,
                status: CallbackStatus::Pending,
                created_at: Instant::now(),
            }).unwrap();
        }

        let turn_incarnation = LIVE_CALLBACKS.lock().get("sess-guard", "cb-turn-1").unwrap().callback_incarnation;
        let turn_mismatch = json!({
            "requestId": "req-mismatch",
            "target": target,
            "callbackId": "cb-turn-1",
            "threadId": "th-1",
            "turnId": "turn-wrong",
            "callbackIncarnation": turn_incarnation,
            "result": { "decision": "accept" }
        });
        let r1 = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&turn_mismatch)
            .send()
            .await
            .unwrap();
        assert_eq!(r1.status(), StatusCode::CONFLICT);

        {
            let mut reg = LIVE_CALLBACKS.lock();
            reg.register(LiveCallbackEntry {
                callback_id: "cb-turn-2".to_string(),
                thread_id: "th-1".to_string(),
                turn_id: "turn-2".to_string(),
                callback_incarnation: 0,
                target: target.clone(),
                kind: CallbackKind::Approval,
                text: None,
                questions: None,
                status: CallbackStatus::Pending,
                created_at: Instant::now(),
            }).unwrap();
        }
        let new_incarnation = LIVE_CALLBACKS.lock().get("sess-guard", "cb-turn-2").unwrap().callback_incarnation;

        let stale_turn_reply = json!({
            "requestId": "req-stale",
            "target": target,
            "callbackId": "cb-turn-1",
            "threadId": "th-1",
            "turnId": "turn-1",
            "callbackIncarnation": turn_incarnation,
            "result": { "decision": "accept" }
        });
        let r4 = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&stale_turn_reply)
            .send()
            .await
            .unwrap();
        assert_eq!(r4.status(), StatusCode::CONFLICT);

        let new_turn_reply = json!({
            "requestId": "req-new",
            "target": target,
            "callbackId": "cb-turn-2",
            "threadId": "th-1",
            "turnId": "turn-2",
            "callbackIncarnation": new_incarnation,
            "result": { "decision": "accept" }
        });
        let r5 = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&new_turn_reply)
            .send()
            .await
            .unwrap();
        assert_eq!(r5.status(), StatusCode::OK);

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_send_without_active_provider_fails_closed() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-noprov".to_string(),
            crate::remote::backend::RemoteSessionDetails {
                session_id: "sess-noprov".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();


        let target = TargetRef {
            host_id: "host-local".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-noprov".to_string(),
        };

        let send_url = format!("http://{}/api/v1/chat/send", addr);
        let body = json!({
            "requestId": "req-send-1",
            "target": target,
            "draft": {
                "text": "Hello agent without provider",
                "attachments": []
            }
        });

        let resp = client
            .post(&send_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let resp_json: Value = resp.json().await.unwrap();
        assert_eq!(resp_json["ok"], false);
        assert_eq!(resp_json["error"]["code"], "UNSUPPORTED");

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_send_with_active_provider_succeeds() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-prov".to_string(),
            crate::remote::backend::RemoteSessionDetails {
                session_id: "sess-prov".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let mock_provider = Arc::new(MockProvider::new());
        register_managed_provider("sess-prov", mock_provider.clone());

        let target = TargetRef {
            host_id: "host-local".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-prov".to_string(),
        };

        let send_url = format!("http://{}/api/v1/chat/send", addr);
        let body = json!({
            "requestId": "req-send-real",
            "target": target,
            "draft": {
                "text": "Test message to real provider",
                "attachments": []
            }
        });

        let resp = client
            .post(&send_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let resp_json: Value = resp.json().await.unwrap();
        assert_eq!(resp_json["ok"], true);
        assert_eq!(resp_json["requestId"], "req-send-real");
        assert_eq!(resp_json["data"]["stage"], "accepted");
        assert_eq!(resp_json["data"]["target"]["backendSessionId"], "sess-prov");

        assert_eq!(mock_provider.sent_inputs.lock().len(), 1);

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_managed_chat_send_attachment_limits_at_consumption_boundary() {
        use crate::remote::attachment_api::{ATTACHMENT_STORE, StagedAttachmentRecord};
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-limit-boundary".to_string(), crate::remote::backend::RemoteSessionDetails {
            session_id: "sess-limit-boundary".to_string(), cols: 80, rows: 24, workspace_id: None,
            worktree_label: None, worktree_path: None, running: true,
        });
        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let provider = Arc::new(MockProvider::new());
        register_managed_provider("sess-limit-boundary", provider.clone());
        let target = TargetRef { host_id: "host-local".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-limit-boundary".into() };
        let send_url = format!("http://{addr}/api/v1/chat/send");
        let mut temp_dirs = Vec::new();
        let mut receipt_ids = Vec::new();
        let mut make_receipt = |size: u64| {
            let dir = std::env::temp_dir().join(format!("ferryx-chat-limit-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("payload.bin");
            let file = std::fs::File::create(&path).unwrap();
            file.set_len(size).unwrap();
            let id = uuid::Uuid::new_v4().to_string();
            receipt_ids.push(id.clone());
            let receipt = AttachmentReceipt { host_id: target.host_id.clone(), attachment_id: id.clone(), sha256: "0".repeat(64), size_bytes: size, media_type: AttachmentMediaType::Text };
            ATTACHMENT_STORE.lock().completed.insert(id.clone(), StagedAttachmentRecord {
                target: target.clone(), attachment_id: id, file_name: "payload.bin".into(), media_type: AttachmentMediaType::Text,
                sha256: receipt.sha256.clone(), size_bytes: size, file_path: path, created_at: Instant::now(),
            });
            temp_dirs.push(dir);
            receipt
        };
        let send = |request_id: &str, receipts: Vec<AttachmentReceipt>| {
            let request_id = request_id.to_string();
            let client = client.clone();
            let url = send_url.clone();
            let token = token.clone();
            let target = target.clone();
            async move {
                client.post(url).bearer_auth(token).json(&json!({
                    "requestId": request_id, "target": target,
                    "draft": {"text":"boundary", "attachments":receipts}
                })).send().await.unwrap()
            }
        };

        let exact: Vec<_> = (0..ATTACHMENT_MAX_FILES_PER_TURN).map(|_| make_receipt(ATTACHMENT_MAX_FILE_BYTES / 2)).collect();
        assert_eq!(exact.iter().map(|r| r.size_bytes).sum::<u64>(), ATTACHMENT_MAX_TURN_BYTES);
        let accepted = send("limit-exact", exact).await;
        assert_eq!(accepted.status(), StatusCode::OK);

        let too_many: Vec<_> = (0..=ATTACHMENT_MAX_FILES_PER_TURN).map(|_| make_receipt(1)).collect();
        let rejected_count = send("limit-count-over", too_many).await;
        assert_eq!(rejected_count.status(), StatusCode::BAD_REQUEST);
        let count_body: Value = rejected_count.json().await.unwrap();
        assert_eq!(count_body["error"]["code"], "INVALID_REQUEST");

        let over_total = vec![make_receipt(ATTACHMENT_MAX_FILE_BYTES), make_receipt(ATTACHMENT_MAX_FILE_BYTES), make_receipt(1)];
        let rejected_total = send("limit-total-over", over_total).await;
        assert_eq!(rejected_total.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let total_body: Value = rejected_total.json().await.unwrap();
        assert_eq!(total_body["error"]["code"], "PAYLOAD_TOO_LARGE");

        let over_file = vec![make_receipt(ATTACHMENT_MAX_FILE_BYTES + 1)];
        let rejected_file = send("limit-file-over", over_file).await;
        assert_eq!(rejected_file.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let file_body: Value = rejected_file.json().await.unwrap();
        assert_eq!(file_body["error"]["code"], "PAYLOAD_TOO_LARGE");

        assert_eq!(provider.sent_inputs.lock().len(), 1);
        let mut store = ATTACHMENT_STORE.lock();
        for receipt_id in receipt_ids { store.completed.remove(&receipt_id); }
        drop(store);
        for dir in temp_dirs { let _ = std::fs::remove_dir_all(dir); }
        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_stop_agent_and_invalidation() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-stop".to_string(),
            crate::remote::backend::RemoteSessionDetails {
                session_id: "sess-stop".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        let mock_provider = Arc::new(MockProvider::new());
        register_managed_provider("sess-stop", mock_provider.clone());

        let target = TargetRef {
            host_id: "host-local".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-stop".to_string(),
        };

        {
            let mut reg = LIVE_CALLBACKS.lock();
            reg.register(LiveCallbackEntry {
                callback_id: "cb-stop-1".to_string(),
                thread_id: "th-stop".to_string(),
                turn_id: "turn-stop".to_string(),
                callback_incarnation: 0,
                target: target.clone(),
                kind: CallbackKind::Approval,
                text: None,
                questions: None,
                status: CallbackStatus::Pending,
                created_at: Instant::now(),
            }).unwrap();
        }

        let stop_url = format!("http://{}/api/v1/chat/stop", addr);
        let resp = client
            .post(&stop_url)
            .bearer_auth(&token)
            .json(&json!({
                "requestId": "req-stop",
                "target": target
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert!(*mock_provider.stopped.lock());

        let reply_url = format!("http://{}/api/v1/chat/reply", addr);
        let reply_resp = client
            .post(&reply_url)
            .bearer_auth(&token)
            .json(&json!({
                "requestId": "req-reply-stale",
                "target": target,
                "callbackId": "cb-stop-1",
                "threadId": "th-stop",
                "turnId": "turn-stop",
                "callbackIncarnation": LIVE_CALLBACKS.lock().get("sess-stop", "cb-stop-1").unwrap().callback_incarnation,
                "result": { "decision": "accept" }
            }))
            .send()
            .await
            .unwrap();

        assert_eq!(reply_resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let reply_json: Value = reply_resp.json().await.unwrap();
        assert_eq!(reply_json["error"]["code"], "UNSUPPORTED");

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }
}
