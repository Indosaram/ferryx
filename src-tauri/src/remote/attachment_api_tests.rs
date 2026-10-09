#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::remote::auth::{DeviceAccessScope, DevicePermission};
    use crate::remote::state::RemoteGatewayState;
    use crate::remote::backend::{RemoteSessionBackend, RemoteSessionDetails};
    use crate::scoped_contracts::{AttachmentMediaType, AttachmentReceipt, TargetRef};
    use axum::http::StatusCode;
    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use futures_util::future::BoxFuture;
    use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Instant};
    use sha2::{Digest, Sha256};
    use crate::scoped_contracts::Epoch;
    use crate::terminal::SessionAttachment;
    use crate::worktree::WorkspaceRegistry;
    use crate::remote::protocol::RemoteActiveDesktopSelection;
    use std::sync::atomic::Ordering;

    #[derive(Default)]
    struct TestSessionBackend {
        sessions: parking_lot::Mutex<HashMap<String, RemoteSessionDetails>>,
    }

    impl RemoteSessionBackend for TestSessionBackend {
        fn describe_session<'a>(
            &'a self,
            session_id: &'a str,
        ) -> BoxFuture<'a, Result<RemoteSessionDetails, String>> {
            let lock = self.sessions.lock();
            let res = lock
                .get(session_id)
                .cloned()
                .ok_or_else(|| format!("Session '{session_id}' not found"));
            Box::pin(async move { res })
        }
        fn list_sessions(&self) -> BoxFuture<'_, Vec<String>> {
            let lock = self.sessions.lock();
            let list = lock.keys().cloned().collect();
            Box::pin(async move { list })
        }
        fn attach_with_sequence<'a>(
            &'a self,
            _session_id: &'a str,
            _after_seq: Option<u64>,
        ) -> BoxFuture<'a, Result<SessionAttachment, String>> {
            Box::pin(async move { Err("unimplemented".into()) })
        }
        fn write_input<'a>(&'a self, _session_id: &'a str, _data: &'a [u8]) -> BoxFuture<'a, Result<(), String>> {
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

    fn temp_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("ferryx-test-att-{}-{}", name, uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
            .exchange_pairing_code(&pin, "test-device")
            .unwrap();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let tokio_listener = tokio::net::TcpListener::from_std(listener).unwrap();

        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let app = attachment_router(Arc::clone(&state));

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
    async fn test_http_auth_and_target_validation() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-valid".to_string(),
            RemoteSessionDetails {
                session_id: "sess-valid".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend.clone());
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let upload_url = format!("http://{}/api/v1/chat/attachments/upload", addr);

        let valid_target = TargetRef {
            host_id: "local-daemon".to_string(),
            owner_id: device_id.clone(),
            epoch: Epoch(1),
            backend_session_id: "sess-valid".to_string(),
        };

        let req_payload = AttachmentUploadChunkRequest {
            target: valid_target.clone(),
            attachment_id: uuid::Uuid::new_v4().to_string(),
            file_name: "test.txt".to_string(),
            media_type: AttachmentMediaType::Text,
            chunk_index: 0,
            total_chunks: 1,
            offset: 0,
            total_bytes: 5,
            data: BASE64_STANDARD.encode(b"hello"),
        };

        let res_no_auth = client
            .post(&upload_url)
            .json(&req_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(res_no_auth.status(), reqwest::StatusCode::UNAUTHORIZED);
        let no_auth_json: serde_json::Value = res_no_auth.json().await.unwrap();
        assert_eq!(no_auth_json["error"]["code"], "UNAUTHORIZED");
        assert_eq!(no_auth_json["error"]["message"], "Authorization token missing or invalid");

        let mut payload_wrong_owner = req_payload.clone();
        payload_wrong_owner.target.owner_id = "rogue-owner".to_string();
        let res_wrong_owner = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&payload_wrong_owner)
            .send()
            .await
            .unwrap();
        assert_eq!(res_wrong_owner.status(), reqwest::StatusCode::FORBIDDEN);
        let wrong_owner_json: serde_json::Value = res_wrong_owner.json().await.unwrap();
        assert_eq!(wrong_owner_json["error"]["code"], "INVALID_OWNER");
        assert_eq!(wrong_owner_json["error"]["message"], "Target owner does not match authenticated device");

        let mut payload_wrong_session = req_payload.clone();
        payload_wrong_session.target.backend_session_id = "ghost-session".to_string();
        let res_wrong_session = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&payload_wrong_session)
            .send()
            .await
            .unwrap();
        assert_eq!(res_wrong_session.status(), reqwest::StatusCode::NOT_FOUND);
        let wrong_session_json: serde_json::Value = res_wrong_session.json().await.unwrap();
        assert_eq!(wrong_session_json, json!({"error":{"code":"SESSION_NOT_FOUND","message":"Target session was not found"}}));

        let mut payload_wrong_epoch = req_payload.clone();
        payload_wrong_epoch.target.epoch = Epoch(999);
        let res_wrong_epoch = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&payload_wrong_epoch)
            .send()
            .await
            .unwrap();
        assert_eq!(res_wrong_epoch.status(), reqwest::StatusCode::CONFLICT);
        let wrong_epoch_json: serde_json::Value = res_wrong_epoch.json().await.unwrap();
        assert_eq!(wrong_epoch_json["error"]["code"], "TARGET_EXPIRED");
        assert_eq!(wrong_epoch_json["error"]["message"], "Target epoch is no longer current");

        let res_valid = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&req_payload)
            .send()
            .await
            .unwrap();
        assert_eq!(res_valid.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = res_valid.json().await.unwrap();
        assert_eq!(body["ok"], true);
        assert_eq!(body["data"]["sizeBytes"], 5);
        assert_eq!(body["data"]["attachmentId"], req_payload.attachment_id);

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_http_chunk_conflicts_and_bounds() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-chunks".to_string(),
            RemoteSessionDetails {
                session_id: "sess-chunks".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: None,
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend.clone());
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let upload_url = format!("http://{}/api/v1/chat/attachments/upload", addr);

        let target = TargetRef {
            host_id: "local-daemon".to_string(),
            owner_id: device_id,
            epoch: Epoch(1),
            backend_session_id: "sess-chunks".to_string(),
        };

        let req_oversized = AttachmentUploadChunkRequest {
            target: target.clone(),
            attachment_id: uuid::Uuid::new_v4().to_string(),
            file_name: "oversized.bin".to_string(),
            media_type: AttachmentMediaType::Pdf,
            chunk_index: 0,
            total_chunks: 1,
            offset: 0,
            total_bytes: ATTACHMENT_MAX_FILE_BYTES + 1,
            data: BASE64_STANDARD.encode(b"oversized"),
        };
        let res_oversized = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&req_oversized)
            .send()
            .await
            .unwrap();
        assert_eq!(
            res_oversized.status(),
            reqwest::StatusCode::PAYLOAD_TOO_LARGE
        );
        let oversized_json: serde_json::Value = res_oversized.json().await.unwrap();
        assert_eq!(oversized_json, json!({"error":{"code":"PAYLOAD_TOO_LARGE","message":"Attachment size limit exceeded"}}));

        let req_bad_index = AttachmentUploadChunkRequest {
            target: target.clone(),
            attachment_id: uuid::Uuid::new_v4().to_string(),
            file_name: "test.txt".to_string(),
            media_type: AttachmentMediaType::Text,
            chunk_index: 2,
            total_chunks: 2,
            offset: 0,
            total_bytes: 10,
            data: BASE64_STANDARD.encode(b"data"),
        };
        let res_bad_index = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&req_bad_index)
            .send()
            .await
            .unwrap();
        assert_eq!(res_bad_index.status(), reqwest::StatusCode::BAD_REQUEST);
        let bad_index_json: serde_json::Value = res_bad_index.json().await.unwrap();
        assert_eq!(bad_index_json, json!({"error":{"code":"INVALID_CHUNK_GEOMETRY","message":"Attachment chunk geometry is invalid"}}));

        let req_offset_overflow = AttachmentUploadChunkRequest {
            target: target.clone(),
            attachment_id: uuid::Uuid::new_v4().to_string(),
            file_name: "test.txt".to_string(),
            media_type: AttachmentMediaType::Text,
            chunk_index: 0,
            total_chunks: 1,
            offset: 8,
            total_bytes: 10,
            data: BASE64_STANDARD.encode(b"overflow_bytes"),
        };
        let res_offset_overflow = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&req_offset_overflow)
            .send()
            .await
            .unwrap();
        assert_eq!(
            res_offset_overflow.status(),
            reqwest::StatusCode::BAD_REQUEST
        );

        let conflict_id = uuid::Uuid::new_v4().to_string();
        let chunk1 = AttachmentUploadChunkRequest {
            target: target.clone(),
            attachment_id: conflict_id.clone(),
            file_name: "conflict.txt".to_string(),
            media_type: AttachmentMediaType::Text,
            chunk_index: 0,
            total_chunks: 2,
            offset: 0,
            total_bytes: 10,
            data: BASE64_STANDARD.encode(b"hello"),
        };
        let res_c1 = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&chunk1)
            .send()
            .await
            .unwrap();
        assert_eq!(res_c1.status(), reqwest::StatusCode::OK);

        let chunk2_conflict = AttachmentUploadChunkRequest {
            target: target.clone(),
            attachment_id: conflict_id.clone(),
            file_name: "conflict.txt".to_string(),
            media_type: AttachmentMediaType::Text,
            chunk_index: 1,
            total_chunks: 3,
            offset: 5,
            total_bytes: 10,
            data: BASE64_STANDARD.encode(b"world"),
        };
        let res_c2 = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&chunk2_conflict)
            .send()
            .await
            .unwrap();
        assert_eq!(res_c2.status(), reqwest::StatusCode::BAD_REQUEST);

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_http_cancellation_and_removed_storage() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-cancel".to_string(),
            RemoteSessionDetails {
                session_id: "sess-cancel".to_string(),
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
        let upload_url = format!("http://{}/api/v1/chat/attachments/upload", addr);
        let cancel_url = format!("http://{}/api/v1/chat/attachments/cancel", addr);

        let target = TargetRef {
            host_id: "local-daemon".to_string(),
            owner_id: device_id,
            epoch: Epoch(1),
            backend_session_id: "sess-cancel".to_string(),
        };

        let attachment_id = uuid::Uuid::new_v4().to_string();
        let chunk1 = AttachmentUploadChunkRequest {
            target: target.clone(),
            attachment_id: attachment_id.clone(),
            file_name: "cancel_me.txt".to_string(),
            media_type: AttachmentMediaType::Text,
            chunk_index: 0,
            total_chunks: 2,
            offset: 0,
            total_bytes: 10,
            data: BASE64_STANDARD.encode(b"chunk1"),
        };

        let res1 = client
            .post(&upload_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&chunk1)
            .send()
            .await
            .unwrap();
        assert_eq!(res1.status(), reqwest::StatusCode::OK);

        let staging_dir = default_attachments_base_dir()
            .join("sess-cancel")
            .join(&attachment_id);
        assert!(staging_dir.exists());

        let cancel_req = AttachmentCancelRequest {
            target: target.clone(),
            attachment_id: attachment_id.clone(),
        };
        let res_cancel = client
            .post(&cancel_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&cancel_req)
            .send()
            .await
            .unwrap();
        assert_eq!(res_cancel.status(), reqwest::StatusCode::OK);

        let cancel_body: serde_json::Value = res_cancel.json().await.unwrap();
        assert_eq!(cancel_body["ok"], true);
        assert_eq!(cancel_body["cleaned"], true);

        assert!(!staging_dir.exists());
        assert!(!ATTACHMENT_STORE.lock().pending.contains_key(&attachment_id));

        let _ = stop_tx.send(());
        let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_cancel_rejects_cross_target_and_epoch_but_allows_owner() {
        let backend = Arc::new(TestSessionBackend::default());
        for session in ["sess-cancel-a", "sess-cancel-b"] {
            backend.sessions.lock().insert(session.to_string(), RemoteSessionDetails {
                session_id: session.to_string(), cols: 80, rows: 24, workspace_id: None,
                worktree_label: None, worktree_path: None, running: true,
            });
        }
        let (state, token, device_id, _, _, _) = setup_test_server(backend);
        let target = TargetRef { host_id: "host".into(), owner_id: device_id.clone(), epoch: Epoch(1), backend_session_id: "sess-cancel-a".into() };
        let attachment_id = uuid::Uuid::new_v4().to_string();
        ATTACHMENT_STORE.lock().pending.insert(attachment_id.clone(), PendingUpload {
            target: target.clone(), file_name: "x.txt".into(), media_type: AttachmentMediaType::Text,
            total_chunks: 1, total_bytes: 1, received_chunks: HashMap::new(), created_at: Instant::now(),
        });
        let mut wrong_target = target.clone();
        wrong_target.backend_session_id = "sess-cancel-b".into();
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        let wrong = cancel_attachment_upload_inner(State(Arc::clone(&state)), headers.clone(), Json(AttachmentCancelRequest { target: wrong_target, attachment_id: attachment_id.clone() })).await.unwrap();
        let wrong_body = axum::body::to_bytes(wrong.into_body(), usize::MAX).await.unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&wrong_body).unwrap()["cleaned"], false);
        let mut wrong_epoch = target.clone(); wrong_epoch.epoch = Epoch(2);
        let epoch_response = cancel_attachment_upload(State(Arc::clone(&state)), headers.clone(), Json(AttachmentCancelRequest { target: wrong_epoch, attachment_id: attachment_id.clone() })).await;
        assert_eq!(epoch_response.status(), StatusCode::CONFLICT);
        let epoch_body = axum::body::to_bytes(epoch_response.into_body(), usize::MAX).await.unwrap();
        let epoch_json: serde_json::Value = serde_json::from_slice(&epoch_body).unwrap();
        assert_eq!(epoch_json["error"]["code"], "TARGET_EXPIRED");
        let own = cancel_attachment_upload_inner(State(state), headers, Json(AttachmentCancelRequest { target, attachment_id: attachment_id.clone() })).await.unwrap();
        let own_body = axum::body::to_bytes(own.into_body(), usize::MAX).await.unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&own_body).unwrap()["cleaned"], true);
        assert!(!ATTACHMENT_STORE.lock().pending.contains_key(&attachment_id));
    }

    #[tokio::test]
    async fn test_upload_rejects_duplicate_chunk_metadata_overlap_and_geometry() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-geometry".into(), RemoteSessionDetails { session_id: "sess-geometry".into(), cols: 80, rows: 24, workspace_id: None, worktree_label: None, worktree_path: None, running: true });
        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{addr}/api/v1/chat/attachments/upload");
        let target = TargetRef { host_id: "host".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-geometry".into() };
        let id = uuid::Uuid::new_v4().to_string();
        let first = AttachmentUploadChunkRequest { target: target.clone(), attachment_id: id.clone(), file_name: "first.txt".into(), media_type: AttachmentMediaType::Text, chunk_index: 0, total_chunks: 2, offset: 0, total_bytes: 10, data: BASE64_STANDARD.encode(b"12345") };
        let response = client.post(&url).bearer_auth(&token).json(&first).send().await.unwrap(); assert_eq!(response.status(), StatusCode::OK);
        let duplicate = AttachmentUploadChunkRequest { data: BASE64_STANDARD.encode(b"abcde"), ..first.clone() };
        let response = client.post(&url).bearer_auth(&token).json(&duplicate).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = response.json().await.unwrap(); assert_eq!(body["error"]["code"], "DUPLICATE_CHUNK_INDEX");
        let changed_metadata = AttachmentUploadChunkRequest { chunk_index: 1, offset: 5, data: BASE64_STANDARD.encode(b"67890"), file_name: "changed.txt".into(), ..first.clone() };
        let response = client.post(&url).bearer_auth(&token).json(&changed_metadata).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let gap_id = uuid::Uuid::new_v4().to_string();
        let gap_first = AttachmentUploadChunkRequest { attachment_id: gap_id.clone(), total_chunks: 2, total_bytes: 10, chunk_index: 0, offset: 0, data: BASE64_STANDARD.encode(b"1234"), ..first.clone() };
        let response = client.post(&url).bearer_auth(&token).json(&gap_first).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let gap_last = AttachmentUploadChunkRequest { attachment_id: gap_id.clone(), chunk_index: 1, offset: 5, data: BASE64_STANDARD.encode(b"67890"), ..gap_first.clone() };
        let response = client.post(&url).bearer_auth(&token).json(&gap_last).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "INVALID_CHUNK_GEOMETRY");
        assert!(!ATTACHMENT_STORE.lock().completed.contains_key(&gap_id));
        assert!(!ATTACHMENT_STORE.lock().pending.contains_key(&gap_id));

        let overlap_id = uuid::Uuid::new_v4().to_string();
        let overlap_first = AttachmentUploadChunkRequest { attachment_id: overlap_id.clone(), total_chunks: 2, total_bytes: 10, chunk_index: 0, offset: 0, data: BASE64_STANDARD.encode(b"12345"), ..first.clone() };
        let response = client.post(&url).bearer_auth(&token).json(&overlap_first).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let overlap_second = AttachmentUploadChunkRequest { attachment_id: overlap_id.clone(), chunk_index: 1, offset: 4, data: BASE64_STANDARD.encode(b"567890"), ..overlap_first.clone() };
        let response = client.post(&url).bearer_auth(&token).json(&overlap_second).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["error"]["code"], "OVERLAPPING_CHUNK_RANGE");
        assert!(!ATTACHMENT_STORE.lock().completed.contains_key(&overlap_id));
        assert!(!ATTACHMENT_STORE.lock().pending.get(&overlap_id).is_some_and(|upload| upload.received_chunks.len() == 2));
        let _ = stop_tx.send(()); let _ = server_handle.await;
    }

    #[tokio::test]
    async fn test_upload_accepts_five_decoded_bytes_with_eight_base64_chars() {
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-b64".into(), RemoteSessionDetails { session_id: "sess-b64".into(), cols: 80, rows: 24, workspace_id: None, worktree_label: None, worktree_path: None, running: true });
        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let req = AttachmentUploadChunkRequest { target: TargetRef { host_id: "host".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-b64".into() }, attachment_id: uuid::Uuid::new_v4().to_string(), file_name: "five.txt".into(), media_type: AttachmentMediaType::Text, chunk_index: 0, total_chunks: 1, offset: 0, total_bytes: 5, data: BASE64_STANDARD.encode(b"12345") };
        assert_eq!(req.data.len(), 8);
        let response = client.post(format!("http://{addr}/api/v1/chat/attachments/upload")).bearer_auth(&token).json(&req).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let _ = stop_tx.send(()); let _ = server_handle.await;
    }

    #[test]
    fn test_turn_attachment_limit_boundaries_and_checked_sum() {
        let sizes = vec![5 * 1024 * 1024; ATTACHMENT_MAX_FILES_PER_TURN];
        assert_eq!(sizes.iter().try_fold(0u64, |sum, size| sum.checked_add(*size)), Some(ATTACHMENT_MAX_TURN_BYTES));
        assert_eq!(sizes.len(), 4);
        assert!(sizes.iter().all(|size| *size <= ATTACHMENT_MAX_FILE_BYTES));
        let one_over = vec![ATTACHMENT_MAX_TURN_BYTES + 1];
        assert!(one_over.iter().try_fold(0u64, |sum, size| sum.checked_add(*size)).unwrap() > ATTACHMENT_MAX_TURN_BYTES);
        assert!(vec![ATTACHMENT_MAX_FILE_BYTES + 1].iter().any(|size| *size > ATTACHMENT_MAX_FILE_BYTES));
        assert_eq!(vec![1u64; ATTACHMENT_MAX_FILES_PER_TURN + 1].len(), 5);
    }

    #[test]
    fn test_chunk_coverage_rejects_gap_and_overlap() {
        let is_exact = |mut ranges: Vec<(u64, u64)>| {
            ranges.sort_unstable_by_key(|(offset, _)| *offset);
            let mut covered = 0u64;
            ranges.iter().all(|(offset, len)| {
                let contiguous = *offset == covered;
                covered = offset.saturating_add(*len);
                contiguous
            }) && covered == 10
        };
        assert!(is_exact(vec![(0, 4), (4, 6)]));
        assert!(!is_exact(vec![(0, 4), (5, 5)]));
        assert!(!is_exact(vec![(0, 6), (5, 5)]));
    }

    #[tokio::test]
    async fn test_http_result_preview_token_and_streaming() {
        let worktree_dir = temp_test_dir("worktree");
        let safe_file = worktree_dir.join("build.log");
        let safe_content = b"Compilation succeeded: 25 tests passed";
        std::fs::write(&safe_file, safe_content).unwrap();

        let outside_dir = temp_test_dir("outside");
        let outside_file = outside_dir.join("secret.key");
        std::fs::write(&outside_file, b"super-secret-key").unwrap();

        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert(
            "sess-preview".to_string(),
            RemoteSessionDetails {
                session_id: "sess-preview".to_string(),
                cols: 80,
                rows: 24,
                workspace_id: None,
                worktree_label: None,
                worktree_path: Some(worktree_dir.clone()),
                running: true,
            },
        );

        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend);
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let token_url = format!("http://{}/api/v1/files/preview/token", addr);

        let target = TargetRef {
            host_id: "local-daemon".to_string(),
            owner_id: device_id,
            epoch: Epoch(1),
            backend_session_id: "sess-preview".to_string(),
        };

        let file_id = register_result_file(target.clone(), safe_file.clone());

        let mint_traversal = ResultPreviewTokenRequest {
            target: target.clone(),
            file_id: "../outside/secret.key".to_string(),
        };
        let res_traversal = client
            .post(&token_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&mint_traversal)
            .send()
            .await
            .unwrap();
        assert_eq!(res_traversal.status(), reqwest::StatusCode::NOT_FOUND);

        let mint_absolute = ResultPreviewTokenRequest {
            target: target.clone(),
            file_id: "/etc/passwd".to_string(),
        };
        let res_abs = client
            .post(&token_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&mint_absolute)
            .send()
            .await
            .unwrap();
        assert_eq!(res_abs.status(), reqwest::StatusCode::NOT_FOUND);

        #[cfg(unix)]
        {
            let symlink_escape = worktree_dir.join("symlink_escape");
            let _ = std::os::unix::fs::symlink(&outside_file, &symlink_escape);
            if symlink_escape.exists() {
                let symlink_id = register_result_file(target.clone(), symlink_escape);
                let mint_symlink = ResultPreviewTokenRequest {
                    target: target.clone(),
                    file_id: symlink_id,
                };
                let res_sym = client
                    .post(&token_url)
                    .header("Authorization", format!("Bearer {token}"))
                    .json(&mint_symlink)
                    .send()
                    .await
                    .unwrap();
                assert_eq!(res_sym.status(), reqwest::StatusCode::FORBIDDEN);
            }
        }

        let mint_valid = ResultPreviewTokenRequest {
            target: target.clone(),
            file_id: file_id.clone(),
        };
        let res_valid = client
            .post(&token_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&mint_valid)
            .send()
            .await
            .unwrap();
        assert_eq!(res_valid.status(), reqwest::StatusCode::OK);
        let mint_body: serde_json::Value = res_valid.json().await.unwrap();
        assert_eq!(mint_body["ok"], true);
        let preview_token = mint_body["token"].as_str().unwrap();

        let preview_url = format!("http://{}/api/v1/files/preview/{}", addr, preview_token);
        let res_stream = client.get(&preview_url).bearer_auth(&token).send().await.unwrap();
        assert_eq!(res_stream.status(), reqwest::StatusCode::OK);
        assert_eq!(
            res_stream.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            res_stream.headers().get("content-disposition").unwrap(),
            "inline"
        );
        assert_eq!(
            res_stream.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
        let body_bytes = res_stream.bytes().await.unwrap();
        assert_eq!(body_bytes.as_ref(), safe_content);

        let res_range = client
            .get(&preview_url)
            .bearer_auth(&token)
            .header("Range", "bytes=0-10")
            .send()
            .await
            .unwrap();
        assert_eq!(res_range.status(), reqwest::StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            res_range.headers().get("content-range").unwrap().to_str().unwrap(),
            format!("bytes 0-10/{}", safe_content.len())
        );
        let range_bytes = res_range.bytes().await.unwrap();
        assert_eq!(range_bytes.as_ref(), &safe_content[0..11]);

        let res_bad_token = client
            .get(format!("http://{}/api/v1/files/preview/ghost-token", addr))
            .send()
            .await
            .unwrap();
        assert_eq!(res_bad_token.status(), reqwest::StatusCode::NOT_FOUND);
        let bad_token_body: serde_json::Value = res_bad_token.json().await.unwrap();
        assert_eq!(bad_token_body["error"]["code"], "UNKNOWN_PREVIEW_ID");
        assert_eq!(bad_token_body["error"]["message"], "Preview identifier was not found or expired");

        let mut different_target = target.clone();
        different_target.backend_session_id = "other-session".into();
        let wrong_target_id = register_result_file(different_target.clone(), safe_file.clone());
        let unknown_target_id = client.post(&token_url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&ResultPreviewTokenRequest { target: target.clone(), file_id: wrong_target_id })
            .send().await.unwrap();
        assert_eq!(unknown_target_id.status(), reqwest::StatusCode::NOT_FOUND);
        let unknown_target_body: serde_json::Value = unknown_target_id.json().await.unwrap();
        assert_eq!(unknown_target_body["error"]["code"], "RESULT_FILE_NOT_FOUND");
        assert_eq!(unknown_target_body["error"]["message"], "Result file identifier was not found for this target");

        let unauthorized = client.get(&preview_url).send().await.unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        std::fs::write(&safe_file, b"replacement with a different length").unwrap();
        let replaced = client.get(&preview_url).bearer_auth(&token).send().await.unwrap();
        assert_eq!(replaced.status(), reqwest::StatusCode::CONFLICT);
        let replaced_body: serde_json::Value = replaced.json().await.unwrap();
        assert_eq!(replaced_body["error"]["code"], "FILE_MODIFIED");
        assert_eq!(replaced_body["error"]["message"], "Preview file changed after the token was issued");

        let identity_file = worktree_dir.join("identity-swap.txt");
        std::fs::write(&identity_file, b"same").unwrap();
        let identity_id = register_result_file(target.clone(), identity_file.clone());
        let minted = client.post(&token_url).bearer_auth(&token)
            .json(&ResultPreviewTokenRequest { target: target.clone(), file_id: identity_id }).send().await.unwrap();
        assert_eq!(minted.status(), reqwest::StatusCode::OK);
        let minted_json: serde_json::Value = minted.json().await.unwrap();
        let identity_token = minted_json["token"].as_str().unwrap();
        std::fs::remove_file(&identity_file).unwrap();
        std::fs::write(&identity_file, b"evil").unwrap();
        let swapped = client.get(format!("http://{}/api/v1/files/preview/{}", addr, identity_token))
            .bearer_auth(&token).send().await.unwrap();
        assert_eq!(swapped.status(), reqwest::StatusCode::FORBIDDEN);
        let swapped_body: serde_json::Value = swapped.json().await.unwrap();
        assert_eq!(swapped_body["error"]["code"], "PERMISSION_DENIED");

        #[cfg(unix)]
        {
            let swap_file = worktree_dir.join("swap.txt");
            let swap_outside = outside_dir.join("swap-secret.txt");
            std::fs::write(&swap_file, b"safe").unwrap();
            std::fs::write(&swap_outside, b"safe").unwrap();
            let swap_id = register_result_file(target.clone(), swap_file.clone());
            let minted = client.post(&token_url).bearer_auth(&token)
                .json(&ResultPreviewTokenRequest { target: target.clone(), file_id: swap_id }).send().await.unwrap();
            assert_eq!(minted.status(), reqwest::StatusCode::OK);
            let minted_json: serde_json::Value = minted.json().await.unwrap();
            let swap_token = minted_json["token"].as_str().unwrap();
            std::fs::remove_file(&swap_file).unwrap();
            std::os::unix::fs::symlink(&swap_outside, &swap_file).unwrap();
            let swapped = client.get(format!("http://{}/api/v1/files/preview/{}", addr, swap_token))
                .bearer_auth(&token).send().await.unwrap();
            assert_eq!(swapped.status(), reqwest::StatusCode::FORBIDDEN);
            let swapped_body: serde_json::Value = swapped.json().await.unwrap();
            assert_eq!(swapped_body["error"]["code"], "PERMISSION_DENIED");
            assert_eq!(swapped_body["error"]["message"], "Preview file is outside the selected worktree or changed");
        }

        let _ = stop_tx.send(());
        let _ = server_handle.await;
        let _ = std::fs::remove_dir_all(&worktree_dir);
        let _ = std::fs::remove_dir_all(&outside_dir);
    }

    #[tokio::test]
    async fn test_preview_selected_workspace_lookup_fails_closed() {
        let root = temp_test_dir("lookup-fail-closed");
        let file = root.join("result.txt");
        std::fs::write(&file, b"valid").unwrap();
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-lookup".into(), RemoteSessionDetails {
            session_id: "sess-lookup".into(), cols: 80, rows: 24, workspace_id: None,
            worktree_label: None, worktree_path: Some(root.clone()), running: true,
        });
        let (state, token, device_id, _, stop_tx, server_handle) = setup_test_server(backend);
        *state.active_selection.write() = Some(RemoteActiveDesktopSelection {
            workspace_id: Some("missing-workspace".into()),
            worktree_slug: Some("missing-worktree".into()),
            ..Default::default()
        });
        let target = TargetRef { host_id: "host".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-lookup".into() };
        let id = register_result_file(target.clone(), file);
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());
        let response = mint_result_preview_token(State(state), headers, Json(ResultPreviewTokenRequest { target, file_id: id })).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "WORKSPACE_NOT_FOUND");
        let _ = stop_tx.send(());
        let _ = server_handle.await;
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn result_file_listing_is_authenticated_path_free_and_empty_without_journal() {
        let root = temp_test_dir("dag-result-list");
        let result_rel = ".omo/senpi-task/dag/results/dag_123/step_1.txt";
        let result_path = root.join(result_rel);
        std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
        std::fs::write(&result_path, b"result").unwrap();
        let runs = root.join(".omo/senpi-task/dag/runs");
        std::fs::create_dir_all(&runs).unwrap();
        std::fs::write(runs.join("dag_123.json"), format!(r#"{{
          "runId":"dag_123","runKey":"run","name":"Run","status":"completed",
          "nodes":[{{"id":"step-1","state":"completed","route":{{"kind":"agent","agent":"test"}},"resultArtifact":{{"relativePath":"{result_rel}","sha256":"abc","bytes":6}}}}],
          "edges":[],"waves":[],"criticalPath":[],"bottlenecks":[]
        }}"#)).unwrap();
        let backend = Arc::new(TestSessionBackend::default());
        backend.sessions.lock().insert("sess-result-list".into(), RemoteSessionDetails {
            session_id: "sess-result-list".into(), cols: 80, rows: 24, workspace_id: None,
            worktree_label: None, worktree_path: Some(root.clone()), running: true,
        });
        let (_state, token, device_id, addr, stop_tx, server_handle) = setup_test_server(backend.clone());
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let url = format!("http://{addr}/api/v1/files/results/list");
        let target = TargetRef { host_id: "host".into(), owner_id: device_id, epoch: Epoch(1), backend_session_id: "sess-result-list".into() };
        let response = client.post(&url).json(&ResultFileListRequest { target: target.clone() }).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let mut wrong_owner = target.clone();
        wrong_owner.owner_id = "wrong-owner".into();
        let response = client.post(&url).bearer_auth(&token).json(&ResultFileListRequest { target: wrong_owner }).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let mut wrong_epoch = target.clone();
        wrong_epoch.epoch = Epoch(9);
        let response = client.post(&url).bearer_auth(&token).json(&ResultFileListRequest { target: wrong_epoch }).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let wrong_session = TargetRef { backend_session_id: "other-session".into(), ..target.clone() };
        let response = client.post(&url).bearer_auth(&token).json(&ResultFileListRequest { target: wrong_session }).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let response = client.post(&url).bearer_auth(&token).json(&ResultFileListRequest { target: target.clone() }).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["files"].as_array().unwrap().len(), 1);
        assert_eq!(body["files"][0]["displayName"], "step_1.txt");
        assert!(body["files"][0]["fileId"].as_str().is_some());
        let serialized = body.to_string();
        for key in ["relativePath", "filePath", "localPath", "directory", "sha256", "bytes"] {
            assert!(!serialized.contains(key));
        }
        assert!(!serialized.contains(root.to_string_lossy().as_ref()));

        let empty_root = temp_test_dir("dag-result-empty");
        backend.sessions.lock().insert("sess-result-empty".into(), RemoteSessionDetails {
            session_id: "sess-result-empty".into(), cols: 80, rows: 24, workspace_id: None,
            worktree_label: None, worktree_path: Some(empty_root.clone()), running: true,
        });
        let empty_target = TargetRef { backend_session_id: "sess-result-empty".into(), ..target };
        let response = client.post(&url).bearer_auth(&token).json(&ResultFileListRequest { target: empty_target }).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value = response.json().await.unwrap();
        assert_eq!(body["files"], serde_json::json!([]));
        let _ = stop_tx.send(()); let _ = server_handle.await;
        let _ = std::fs::remove_dir_all(root); let _ = std::fs::remove_dir_all(empty_root);
    }

    #[test]
    fn test_attachment_filename_validation() {
        assert_eq!(
            validate_attachment_file_name("diagram.png").unwrap(),
            "diagram.png"
        );
        assert_eq!(
            validate_attachment_file_name("설계_다이어그램.png").unwrap(),
            "설계_다이어그램.png"
        );
        assert_eq!(
            validate_attachment_file_name("résumé.txt").unwrap(),
            "résumé.txt"
        );
        assert_eq!(
            validate_attachment_file_name("a..b.png").unwrap(),
            "a..b.png"
        );
        assert_eq!(
            validate_attachment_file_name("archive.v1.0.tar.gz").unwrap(),
            "archive.v1.0.tar.gz"
        );

        assert!(validate_attachment_file_name("").is_err());
        assert!(validate_attachment_file_name("   ").is_err());
        assert!(validate_attachment_file_name("foo/bar.png").is_err());
        assert!(validate_attachment_file_name("foo\\bar.png").is_err());
        assert!(validate_attachment_file_name(".").is_err());
        assert!(validate_attachment_file_name("..").is_err());
        assert!(validate_attachment_file_name("bad\0file.png").is_err());
        assert!(validate_attachment_file_name("bad\x1bfile.png").is_err());
        assert!(validate_attachment_file_name(&"a".repeat(256)).is_err());
    }

    #[test]
    fn test_is_absolute_token_cross_platform() {
        assert!(is_absolute_token("/root/secret"));
        assert!(is_absolute_token("\\Windows\\System32"));
        assert!(is_absolute_token("C:\\Windows\\System32"));
        assert!(is_absolute_token("D:/project/file"));
        assert!(!is_absolute_token("file.txt"));
        assert!(!is_absolute_token("sub/dir/file.txt"));
        assert!(!is_absolute_token("sub\\dir\\file.txt"));
    }

    #[test]
    fn test_resolve_contained_preview_path_safe_and_hostile() {
        let root = temp_test_dir("jail-root");
        let safe_file = root.join("report.txt");
        std::fs::write(&safe_file, b"safe report").unwrap();

        let resolved = resolve_contained_preview_path(&root, "report.txt").unwrap();
        assert_eq!(resolved, std::fs::canonicalize(&safe_file).unwrap());

        assert!(resolve_contained_preview_path(&root, "").is_err());
        assert!(resolve_contained_preview_path(&root, "../outside.txt").is_err());
        assert!(resolve_contained_preview_path(&root, "..\\outside.txt").is_err());
        assert!(resolve_contained_preview_path(&root, "/etc/passwd").is_err());
        assert!(resolve_contained_preview_path(&root, "C:\\autoexec.bat").is_err());
        assert!(resolve_contained_preview_path(&root, "~/secret").is_err());

        let outside_dir = temp_test_dir("outside");
        let outside_file = outside_dir.join("secret.txt");
        std::fs::write(&outside_file, b"secret content").unwrap();

        #[cfg(unix)]
        {
            let hostile_symlink = root.join("symlink_escape");
            let _ = std::os::unix::fs::symlink(&outside_file, &hostile_symlink);
            if hostile_symlink.exists() {
                assert!(resolve_contained_preview_path(&root, "symlink_escape").is_err());
            }
        }

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside_dir);
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_verbatim_prefix_normalization_matches_both_forms() {
        let prefixed = PathBuf::from(r"\\?\C:\worktree\result.txt");
        let plain = PathBuf::from(r"C:\worktree\result.txt");
        assert_eq!(normalized_canonical_path(&prefixed), normalized_canonical_path(&plain));

        let unc_prefixed = PathBuf::from(r"\\?\UNC\server\share\worktree\result.txt");
        let unc_plain = PathBuf::from(r"\\server\share\worktree\result.txt");
        assert_eq!(normalized_canonical_path(&unc_prefixed), normalized_canonical_path(&unc_plain));
    }

    #[test]
    fn test_staged_attachments_verification_contract() {
        let target = TargetRef {
            host_id: "host-1".to_string(),
            owner_id: "owner-1".to_string(),
            epoch: Epoch(1),
            backend_session_id: "sess-1".to_string(),
        };

        let temp_dir = temp_test_dir("staging-test");
        let file_path = temp_dir.join("test.txt");
        let content = b"Ferryx Staged Attachment Content";
        std::fs::write(&file_path, content).unwrap();

        let mut hasher = Sha256::new();
        hasher.update(content);
        let sha256 = format!("{:x}", hasher.finalize());

        let attachment_id = uuid::Uuid::new_v4().to_string();

        {
            let mut store = ATTACHMENT_STORE.lock();
            store.completed.insert(
                attachment_id.clone(),
                StagedAttachmentRecord {
                    target: target.clone(),
                    attachment_id: attachment_id.clone(),
                    file_name: "test.txt".to_string(),
                    media_type: AttachmentMediaType::Text,
                    sha256: sha256.clone(),
                    size_bytes: content.len() as u64,
                    file_path: file_path.clone(),
                    created_at: Instant::now(),
                },
            );
        }

        let staging = GatewayStagedAttachments;

        let valid_receipt = AttachmentReceipt {
            host_id: target.host_id.clone(),
            attachment_id: attachment_id.clone(),
            sha256: sha256.clone(),
            size_bytes: content.len() as u64,
            media_type: AttachmentMediaType::Text,
        };

        let result = crate::ferryx_scope::chat::attachments::StagedAttachments::verified_input(
            &staging,
            &target,
            &valid_receipt,
        );
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["type"], "attachment");
        assert_eq!(val["attachmentId"], attachment_id);

        let wrong_target = TargetRef {
            host_id: "host-rogue".to_string(),
            owner_id: "owner-1".to_string(),
            epoch: Epoch(1),
            backend_session_id: "sess-1".to_string(),
        };
        let bad_target_res =
            crate::ferryx_scope::chat::attachments::StagedAttachments::verified_input(
                &staging,
                &wrong_target,
                &valid_receipt,
            );
        assert!(bad_target_res.is_err());

        let corrupted_receipt = AttachmentReceipt {
            host_id: target.host_id.clone(),
            attachment_id: attachment_id.clone(),
            sha256: "0000000000000000000000000000000000000000000000000000000000000000"
                .to_string(),
            size_bytes: content.len() as u64,
            media_type: AttachmentMediaType::Text,
        };
        let bad_hash_res =
            crate::ferryx_scope::chat::attachments::StagedAttachments::verified_input(
                &staging,
                &target,
                &corrupted_receipt,
            );
        assert!(bad_hash_res.is_err());

        let mut store = ATTACHMENT_STORE.lock();
        store.completed.remove(&attachment_id);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
